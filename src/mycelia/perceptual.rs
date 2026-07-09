//! The perceptual speed limit — how fast the mold may grow while staying **below a human's ability to
//! notice movement**, and the morph-target contract for the death cap fruit body.
//!
//! This module is pure arithmetic: no ECS, no GPU, no I/O. It exists so the invariant that governs every
//! autonomous motion in `mycelia` can be *proved* in a unit test rather than eyeballed in a playtest.
//!
//! # The threshold
//!
//! Two independent detectors have to be defeated, and they have different limits.
//!
//! **Motion energy.** The slowest motion a human can see depends critically on whether a *stationary
//! reference* is available. Against a blank field the threshold is ~10–20 arcmin/s; next to a static edge
//! it collapses to ~1–2 arcmin/s. See Leibowitz (1955), "Effect of reference lines on the discrimination of
//! movement," JOSA 45:829 (10.1364/josa.45.000829); Shaffer & Wallach (1966), "Extent-of-motion thresholds
//! under subject-relative and object-relative conditions" (10.3758/bf03207425); reviewed with the
//! displacement-threshold framing in Nakayama (1985), "Biological image motion processing: a review,"
//! Vision Research 25:625 (10.1016/0042-6989(85)90171-3). The mold is always adjacent to a static dungeon
//! floor and the mushrooms stand on it, so the strict **object-relative** number is the one that binds.
//!
//! **Temporal contrast.** A mushroom fading in, or a patch of mat brightening, is a luminance change rather
//! than motion. Sensitivity to slow modulation collapses below ~0.1 Hz — Kelly (1979), "Motion and vision
//! II: stabilized spatio-temporal threshold surface," JOSA 69:1340 (10.1364/josa.69.001340) — and,
//! decisively, sufficiently *gradual* changes go unnoticed even by observers actively hunting for them,
//! with no visual disruption at all: Simons, Franconeri & Reimer (2000), "Change blindness in the absence
//! of a visual disruption," Perception 29:1143 (10.1068/p3104); mechanism in Frey et al. (2024), "Memory
//! representations during slow change blindness," J. Vision 24(9):8 (10.1167/jov.24.9.8).
//!
//! # Why an orthographic camera makes this exact
//!
//! The game camera is orthographic with `ScalingMode::FixedVertical` (see `crate::camera`), so world→screen
//! scale is a constant independent of depth. Visual angle per world unit is therefore just
//! `fov_deg_v / viewport_height`, and the whole budget is one division — see [`v_max`]. There is no
//! per-object distance term, and no worst case to guess at.
//!
//! # What the limit does *not* cover
//!
//! [`v_max`] gates **autonomous** motion — growth the mold performs on its own. Change *caused by an agent*
//! (a crab taking a bite, a boot crushing a cap) is meant to be seen and is deliberately exempt. That is
//! the same principle already at work in the module: the mold hides from a gaze but visibly scatters from
//! footsteps.

/// Vertical visual angle the game window subtends at the player's eye: a 27" panel at ~60 cm shows about
/// 31°. This is the one genuinely unknowable number here — it depends on the player's desk — so it is a
/// config dial (`mycelia.screen_fov_deg_v`) rather than a constant, and this value is only the default the
/// shipped RON carries.
pub const NOMINAL_SCREEN_FOV_DEG_V: f32 = 30.0;

/// Slowest motion a human reliably detects next to a stationary reference, in degrees per second.
/// `0.02 deg/s` = 1.2 arcmin/s — the conservative end of the object-relative range (Leibowitz 1955).
/// Shipped in the RON as `mycelia.motion_threshold_deg_per_s`; this is the documented default.
pub const NOMINAL_MOTION_THRESHOLD_DEG_PER_S: f32 = 0.02;

/// No opacity or albedo transition may complete faster than this. Gradual changes spread over ≥12 s are not
/// noticed even by observers instructed to look for them (Simons, Franconeri & Reimer 2000, 10.1068/p3104).
/// Motion has its own, much tighter budget ([`v_max`]); this bounds the *non-moving* half of the signal.
pub const MIN_APPEARANCE_RAMP_SECS: f32 = 12.0;

/// The `growth` values at which the death cap's morph targets were baked, from the asset's operating manual
/// (`death_cap_procedural/CLAUDE.md`). Index 0 is the **basis** (the sealed egg, all weights zero); the six
/// remaining entries correspond one-for-one with the six morph targets `grow_012 … grow_100`.
///
/// If `STAGES` changes in `mushroom_gen.py`, this and [`STAGE_MAX_DISP`] must both be re-derived.
pub const STAGE_T: [f32; 7] = [0.0, 0.12, 0.28, 0.45, 0.62, 0.80, 1.0];

/// Maximum vertex chord length, in **metres at the asset's native scale**, across each of the six morph
/// segments. Because glTF morph blending is linear within a segment, a vertex traces a straight chord and
/// its speed is exactly `chord / segment_duration` — which is what makes the speed limit in [`growth_rate`]
/// exact rather than approximate.
///
/// Measured directly from the generator, over all 1,315 vertices:
///
/// ```text
/// cd death_cap_procedural/src && python3 -c "
/// import math, mushroom_gen as m
/// T=[0.0,0.12,0.28,0.45,0.62,0.80,1.0]; V=[m.build(t)[0] for t in T]
/// for k in range(6):
///     print(max(math.dist(a,b) for a,b in zip(V[k],V[k+1])))"
/// ```
///
/// Sums to 11.40 cm of vertex travel from egg to adult. Note how lopsided it is: the sealed-egg segment
/// moves 0.6 **mm**, the veil rupture moves 3.06 cm. A speed limit on vertices therefore spends almost all
/// of its time exactly where the interesting geometry happens, for free.
pub const STAGE_MAX_DISP: [f32; 6] = [0.00060, 0.01978, 0.03057, 0.02778, 0.02397, 0.01134];

/// Height of the sealed egg (metres, native scale) — the distance a fruit body must rise out of the mat
/// before any of it is above the floor. A 4.85 cm egg *appearing* is an enormous change signal, so the body
/// spawns sunk by this much and is raised at [`v_max`] like every other autonomous motion. It is also what
/// a primary hyphal knot really does: it forms *within* the mycelium and pushes up.
pub const EGG_HEIGHT_M: f32 = 0.0485;

/// The `growth` value past which the universal veil has ruptured and the cap is expanding. Below this the
/// egg is sealed; above it the mushroom is recognisably a mushroom. Used as the light gate (a primordium
/// only opens once seen) and as the amatoxin threshold — amatoxins concentrate in the pileus rather than
/// the stipe or volva (Enjalbert et al. 1993, Toxicon 31:803, 10.1016/0041-0101(93)90386-w), so a body is
/// only poisonous once it has a cap to hold them.
pub const VEIL_RUPTURE_T: f32 = STAGE_T[3];

/// The autonomous-motion budget, in **world units per second**.
///
/// `threshold_deg_per_s` is the psychophysical limit; `fov_deg_v` and `viewport_height` describe the
/// orthographic projection (degrees of visual angle, and world units, spanned by the window's height).
/// Since the projection is orthographic, `viewport_height / fov_deg_v` is exactly world units per degree.
///
/// At the shipped defaults this is 3.33 mm/s fully zoomed in (`viewport_height = camera::MIN_ZOOM = 5.0`)
/// and 22.7 mm/s fully zoomed out — so growth runs ~7× faster when the player cannot resolve it anyway.
/// One formula, evaluated against the live zoom; no worst case is hard-coded.
pub fn v_max(threshold_deg_per_s: f32, fov_deg_v: f32, viewport_height: f32) -> f32 {
    threshold_deg_per_s * viewport_height / fov_deg_v
}

/// Which morph segment `growth` falls in: the `k` such that `STAGE_T[k] <= growth <= STAGE_T[k+1]`.
/// Saturates at the ends, so `growth` outside `[0,1]` is clamped rather than panicking.
pub fn segment_index(growth: f32) -> usize {
    let g = growth.clamp(0.0, 1.0);
    // Six segments; the last one owns g == 1.0.
    (0..6).find(|&k| g <= STAGE_T[k + 1]).unwrap_or(5)
}

/// `d(growth)/dt` that holds the fastest-moving vertex at exactly `v_max`.
///
/// Within segment `k` the fastest vertex travels `STAGE_MAX_DISP[k] * body_scale` metres while `growth`
/// crosses `STAGE_T[k+1] - STAGE_T[k]`. Setting that vertex's speed to `v_max` and solving:
///
/// ```text
/// segment_duration = STAGE_MAX_DISP[k] * body_scale / v_max
/// dgrowth/dt       = (STAGE_T[k+1] - STAGE_T[k]) / segment_duration
/// ```
///
/// Always finite: every entry of [`STAGE_MAX_DISP`] is strictly positive, and `body_scale` is validated
/// `> 0`. The returned rate is unsigned — callers apply the biology gate (which may be negative, when a
/// primordium aborts or something takes a bite).
pub fn growth_rate(growth: f32, body_scale: f32, v_max: f32) -> f32 {
    let k = segment_index(growth);
    let span = STAGE_T[k + 1] - STAGE_T[k];
    let duration = STAGE_MAX_DISP[k] * body_scale / v_max;
    span / duration
}

/// Seconds for one body to go from sealed egg to adult at a fixed `v_max`, ignoring the emergence rise.
/// Only used for diagnostics and tests — the live clock re-evaluates `v_max` every frame against the zoom.
pub fn egg_to_adult_secs(body_scale: f32, v_max: f32) -> f32 {
    STAGE_MAX_DISP.iter().map(|d| d * body_scale / v_max).sum()
}

/// `growth` in `[0,1]` → the six morph-target weights, in target order (`grow_012 … grow_100`).
///
/// Transcribed from the asset's operating manual. In the first segment the **basis** carries the
/// remainder, so the six weights sum to less than 1 there. That is correct, not a bug: glTF morphs are
/// additive (`final = basis + Σ wᵢ·(stageᵢ − basis)`).
///
/// At most two targets are ever active at once. Interpolating egg→adult directly instead would drive the
/// cap straight through the closed volva and the veil would never open — the intermediate stages are what
/// keep the geometry on the real growth path.
pub fn stage_weights(growth: f32) -> [f32; 6] {
    let g = growth.clamp(0.0, 1.0);
    let mut w = [0.0; 6];
    let k = segment_index(g);
    let (a, b) = (STAGE_T[k], STAGE_T[k + 1]);
    let u = ((g - a) / (b - a)).clamp(0.0, 1.0);
    if k > 0 {
        w[k - 1] = 1.0 - u; // the stage we are leaving
    }
    w[k] = u; // the stage we are approaching
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{MAX_ZOOM, MIN_ZOOM};

    const THRESH: f32 = NOMINAL_MOTION_THRESHOLD_DEG_PER_S;
    const FOV: f32 = NOMINAL_SCREEN_FOV_DEG_V;
    const SHIPPED_SCALE: f32 = 4.0;

    /// **The invariant.** For every morph segment and every zoom the player can reach, the fastest vertex
    /// in the mesh must move no faster than the motion-detection threshold. This is the whole point of the
    /// module, proved arithmetically rather than by playtest.
    ///
    /// Vertex speed within a segment is `chord / duration`, and `duration = span / growth_rate`, so the
    /// speed is `chord * growth_rate / span`. It must equal `v_max` exactly.
    #[test]
    fn fastest_vertex_never_exceeds_the_motion_threshold() {
        for steps in 0..=32u32 {
            let viewport = MIN_ZOOM + (MAX_ZOOM - MIN_ZOOM) * (steps as f32 / 32.0);
            let budget = v_max(THRESH, FOV, viewport);
            for k in 0..6 {
                // Sample strictly inside the segment so `segment_index` lands on `k`.
                let g = STAGE_T[k] + 0.5 * (STAGE_T[k + 1] - STAGE_T[k]);
                assert_eq!(segment_index(g), k, "sample fell outside segment {k}");

                let rate = growth_rate(g, SHIPPED_SCALE, budget);
                let span = STAGE_T[k + 1] - STAGE_T[k];
                let vertex_speed = STAGE_MAX_DISP[k] * SHIPPED_SCALE * rate / span;

                assert!(
                    vertex_speed <= budget * (1.0 + 1e-4),
                    "segment {k} at viewport {viewport}: vertex {vertex_speed} m/s exceeds budget {budget} m/s",
                );
            }
        }
    }

    /// The budget scales linearly with zoom-out and is strictly positive everywhere in range. A player
    /// zoomed all the way in gets the tightest limit, which is the case the design is anchored on.
    #[test]
    fn budget_is_monotonic_in_zoom_and_matches_the_documented_numbers() {
        let tight = v_max(THRESH, FOV, MIN_ZOOM);
        let loose = v_max(THRESH, FOV, MAX_ZOOM);
        assert!(tight > 0.0 && loose > tight);
        // 0.02 * 5 / 30 = 3.333 mm/s; 0.02 * 34 / 30 = 22.67 mm/s.
        assert!((tight - 0.003_333).abs() < 1e-5, "got {tight}");
        assert!((loose - 0.022_667).abs() < 1e-5, "got {loose}");
    }

    /// The documented egg→adult durations. These are the numbers a reviewer can check against a stopwatch.
    #[test]
    fn egg_to_adult_takes_the_documented_time() {
        // 11.40 cm of vertex travel at the asset's native scale.
        let travel: f32 = STAGE_MAX_DISP.iter().sum();
        assert!((travel - 0.1140).abs() < 1e-4, "travel = {travel}");

        // At the shipped body_scale of 4.0: 0.1140 m x 4 = 45.6 cm of vertex travel.
        let secs = |viewport| egg_to_adult_secs(SHIPPED_SCALE, v_max(THRESH, FOV, viewport));
        assert!((secs(MIN_ZOOM) - 136.8).abs() < 1.0, "max zoom-in: {}", secs(MIN_ZOOM));
        assert!((secs(12.0) - 57.0).abs() < 1.0, "startup zoom: {}", secs(12.0));
        assert!((secs(MAX_ZOOM) - 20.1).abs() < 1.0, "max zoom-out: {}", secs(MAX_ZOOM));
    }

    /// The asset contract: at most two targets active, weights non-negative, and the basis carries the
    /// remainder in the first segment (so the six weights sum to < 1 there, and to exactly 1 elsewhere).
    #[test]
    fn stage_weights_activate_at_most_two_targets() {
        for i in 0..=200u32 {
            let g = i as f32 / 200.0;
            let w = stage_weights(g);
            let active = w.iter().filter(|x| **x > 0.0).count();
            assert!(active <= 2, "growth {g} activated {active} targets: {w:?}");
            assert!(w.iter().all(|x| (0.0..=1.0).contains(x)), "growth {g} -> {w:?}");

            let sum: f32 = w.iter().sum();
            if g < STAGE_T[1] {
                assert!(sum <= 1.0 + 1e-5, "basis must carry the remainder: {sum}");
            } else {
                assert!((sum - 1.0).abs() < 1e-4, "growth {g}: weights sum to {sum}, want 1.0");
            }
        }
    }

    /// The endpoints must be exact: `growth = 0` is the pure basis (the sealed egg, no target active), and
    /// `growth = 1` is the final target alone. Anything else and the mushroom never fully closes or opens.
    #[test]
    fn stage_weights_endpoints_are_exact() {
        assert_eq!(stage_weights(0.0), [0.0; 6]);
        assert_eq!(stage_weights(1.0), [0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        // Out-of-range input clamps rather than panicking or extrapolating past the adult.
        assert_eq!(stage_weights(-1.0), stage_weights(0.0));
        assert_eq!(stage_weights(2.0), stage_weights(1.0));
    }

    /// Every baked stage `t` must reproduce that stage exactly (weight 1 on it, 0 elsewhere) — otherwise
    /// the blend passes *through* a stage rather than landing on it, and the volva-seal invariant that the
    /// generator guarantees at those sample points no longer applies mid-blend.
    #[test]
    fn baked_stage_samples_reproduce_their_stage_exactly() {
        for (k, &t) in STAGE_T.iter().enumerate().skip(1) {
            let w = stage_weights(t);
            assert!((w[k - 1] - 1.0).abs() < 1e-5, "stage t={t} -> {w:?}");
            assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-5, "stage t={t} -> {w:?}");
        }
    }

    /// The clock must linger where the geometry moves. Segment 2 (the veil rupture, 3.06 cm of travel) has
    /// to be the slowest in `growth`-per-second, and segment 0 (the sealed egg, 0.6 mm) the fastest.
    #[test]
    fn the_clock_lingers_on_the_veil_rupture() {
        let budget = v_max(THRESH, FOV, MIN_ZOOM);
        let rate = |k: usize| {
            let g = STAGE_T[k] + 0.5 * (STAGE_T[k + 1] - STAGE_T[k]);
            growth_rate(g, SHIPPED_SCALE, budget)
        };
        let rates: Vec<f32> = (0..6).map(rate).collect();
        let slowest = rates.iter().copied().fold(f32::INFINITY, f32::min);
        let fastest = rates.iter().copied().fold(0.0f32, f32::max);
        assert_eq!(rates[2], slowest, "veil rupture should be the slowest segment: {rates:?}");
        assert_eq!(rates[0], fastest, "the sealed egg should be the fastest segment: {rates:?}");
        assert!(rates.iter().all(|r| r.is_finite() && *r > 0.0), "{rates:?}");
    }

    /// `segment_index` must cover `[0,1]` with no gaps and no panics at the boundaries.
    #[test]
    fn segment_index_is_total() {
        assert_eq!(segment_index(0.0), 0);
        assert_eq!(segment_index(1.0), 5);
        // NaN clamps to the low end rather than escaping the range (`clamp` returns the min for NaN).
        assert!(segment_index(f32::NAN) < 6);
        for i in 0..=100u32 {
            let k = segment_index(i as f32 / 100.0);
            assert!(k < 6);
        }
        // Exact stage boundaries belong to the segment they close.
        assert_eq!(segment_index(STAGE_T[1]), 0);
        assert_eq!(segment_index(STAGE_T[1] + 1e-6), 1);
    }
}
