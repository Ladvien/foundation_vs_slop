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
//!
//! # The clock these budgets are denominated in
//!
//! Every rate here is **per second of `Time<Virtual>`** — the gameplay clock. At ×1 that is a real second and
//! the thresholds mean what the psychophysics says they mean. Above ×1 they do not, and that is the entire
//! purpose of the speed ladder: fast-forward deliberately lifts the mold's autonomous motion above the
//! detection threshold so a player who *wants* to watch the colony spread can. A pause drives the clock to
//! zero and the mold stops, which is what "the sim is frozen" ought to mean.

use bevy::math::{Vec2, Vec3};

use crate::util::hash01_u32;

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

/// Move `current` toward `target` so that a full `0 → 1` transition can never complete faster than
/// `ramp_secs`. The one rate limiter for every *non-moving* signal in this module — a fruit body's albedo
/// as it matures, and the mat's glow as it flinches from a gaze.
///
/// `dt` and `ramp_secs` must be in the same clock — virtual seconds, throughout this module. Symmetric (it
/// limits fades in and out alike), monotone, and a no-op at `dt == 0`, so a paused game holds its shading
/// exactly where it was, and ×16 completes the ramp sixteen times sooner in wall-clock terms.
///
/// A non-positive `ramp_secs` would divide by zero and teleport the value; callers pass
/// [`MIN_APPEARANCE_RAMP_SECS`], and `validate_config` rejects a non-positive ramp at startup. Guarding
/// here as well would be a second, silent path — so this simply documents the contract.
pub fn slew(current: f32, target: f32, dt: f32, ramp_secs: f32) -> f32 {
    let step = dt / ramp_secs;
    current + (target - current).clamp(-step, step)
}

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
/// Measured from **the shipped `.glb` itself**, over all 1,379 vertices, by rebuilding each stage as
/// `basis + delta` (the deltas are sparse accessors) and taking the longest chord between consecutive
/// stages. Not from `mushroom_gen.py`: the generator is a separate artifact that has already changed its
/// `build()` signature once, and the mesh the game loads is the only thing this limit may describe.
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

/// Apex height of each baked stage, metres at native scale, matching [`STAGE_T`] index for index. Printed by
/// the asset's own `inspect_glb.py`, which rebuilds each stage from `basis + delta`.
pub const STAGE_HEIGHT_M: [f32; 7] = [0.0485, 0.0484, 0.0627, 0.0933, 0.1192, 0.1345, 0.1393];

/// Adult height, metres at native scale.
pub const ADULT_HEIGHT_M: f32 = STAGE_HEIGHT_M[6];

/// Adult cap (pileus) radius, metres at native scale. Four times the volva's, which is the entire reason a
/// mushroom whose *base* clears a wall can still drive its *cap* straight through it.
pub const CAP_RADIUS_M: f32 = 0.0560;

/// Adult volva radius, metres at native scale. The body's actual footprint on the floor.
pub const VOLVA_RADIUS_M: f32 = 0.0230;

/// The stipe's bending zone, metres at native scale: `[BEND_LO_M, BEND_HI_M]`.
///
/// Tropic bending in a mushroom stem is driven by *differential cell elongation*, and the extension is
/// concentrated in the **upper 20–30% of the stem** — the outer flank's cells end up four to five times
/// longer than the inner flank's (Greening, Sánchez & Moore 1997, "Coordinated cell elongation alone drives
/// tropic bending in stems of the mushroom fruit body of *Coprinus cinereus*", Can. J. Bot. 75:1174,
/// 10.1139/b97-830). The stipe of this mesh spans 2.18–11.80 cm, so its upper 30% starts at 8.91 cm; the
/// zone closes at the cap's underside, 11.80 cm.
///
/// Above `BEND_HI_M` the profile saturates, so the cap rides the bent stem as a **rigid, still-level**
/// disc rather than shearing with it. That is not a shortcut: the hymenophore is positively gravitropic and
/// re-levels independently of the stem (Moore 1991, "Perception and response to gravity in higher fungi",
/// New Phytol. 117:3, 10.1111/j.1469-8137.1991.tb00940.x).
///
/// Below `BEND_LO_M` the profile is zero, so the volva stays planted and an egg or a young button is
/// perfectly straight. It straightens out of the biology rather than out of a special case: a stipe that has
/// not yet grown into the bending zone cannot bend.
///
/// **These two constants are duplicated in `mycelia_fruit.wgsl`.** They must agree, or the CPU's growth
/// budget (which folds the bend's travel into the speed limit, see [`STAGE_BEND_FRACTION`]) would describe
/// a different curve from the one the vertex shader draws.
pub const BEND_LO_M: f32 = 0.0891;
pub const BEND_HI_M: f32 = 0.1180;

/// Hard ceiling on a body's apex deflection, metres at native scale — 35% of the adult height. Past this the
/// stipe reads as broken rather than bent, and the speed limit starts charging more for the bend than for
/// the entire morph.
pub const MAX_BEND_M: f32 = 0.35 * ADULT_HEIGHT_M;

/// Hard ceiling on a body's **tilt**: horizontal drift per unit of height, so `0.22` is a lean of
/// `atan(0.22)` ≈ 12.4°. Drawn uniformly, so a flush averages about 6° off plumb — enough that no two
/// mushrooms read as the same model at different growth stages, which is exactly what they did at 9°. Unlike the bend this is a *linear* term, applied from the ground up, so it is the
/// body's overall growth angle rather than a curve in its stem — the volva stays seated because the
/// displacement is zero at `y = 0`.
///
/// The youngest fruit-body initials grow perpendicular to their substratum, and negative gravitropism only
/// takes over later (Moore 1991, 10.1111/j.1469-8137.1991.tb00940.x); no stem ends up exactly plumb.
pub const MAX_TILT: f32 = 0.22;

/// `|Δheight|` across each morph segment, metres at native scale, from [`STAGE_HEIGHT_M`]. A tilted body's
/// apex drifts sideways by `tilt × Δheight` as it grows, which is vertex travel the speed limit must charge
/// for exactly as it charges for the bend.
pub const STAGE_HEIGHT_DELTA: [f32; 6] = [0.0001, 0.0143, 0.0306, 0.0259, 0.0153, 0.0048];

/// The adult body's silhouette: the largest radius (metres, native scale) found in each of 16 equal slices
/// of `[0, ADULT_HEIGHT_M]`. Read straight off the shipped `.glb`, taking the maximum `hypot(x, z)` per
/// slice and linearly interpolating the slices that fall between vertex rings.
///
/// This is what makes wall clearance solvable rather than guessed. Two facts fall out of it:
///
/// - Everything wide is high. The 5.60 cm cap lives in the top three slices, where [`bend_profile`] has
///   saturated at `1.0` — so a bend moves it one-for-one.
/// - The widest thing that **cannot** be bent (`bend_profile < 0.05`) is the volva, at 2.30 cm. The annulus
///   at 9.14 cm is only 1.24 cm across.
///
/// So a body's base must clear 2.30 cm of wall and no more, and its cap — four times wider — is carried
/// clear by curving the stem. A keep-out radius sized for the cap would have banished mushrooms from
/// exactly the damp skirting where the mold pools and a real flush appears.
pub const RADIUS_PROFILE: [f32; 16] = [
    0.0184, 0.0225, 0.0230, 0.0142, 0.0123, 0.0106, 0.0099, 0.0092, 0.0082, 0.0103, 0.0124, 0.0088,
    0.0070, 0.0560, 0.0533, 0.0396,
];

/// Centre height (metres, native scale) of `RADIUS_PROFILE[i]`.
pub fn radius_slice_height(i: usize) -> f32 {
    (i as f32 + 0.5) * ADULT_HEIGHT_M / RADIUS_PROFILE.len() as f32
}

/// Below this, [`bend_profile`] is too weak to move a ring meaningfully — the base must clear it instead.
pub const BENDABLE_MIN_PROFILE: f32 = 0.05;

/// What fraction of a body's total bend is laid down during each morph segment.
///
/// The bend is a function of the stipe's *height*, so it develops as the stipe grows through
/// `[BEND_LO_M, BEND_HI_M]`. That is extra vertex travel on top of the morph's own chord, and if it were not
/// charged to the speed limit the mushroom would visibly swing over as it matured. Almost all of it lands in
/// segment 3 (`growth` 0.45 → 0.62), where the apex climbs 9.33 cm → 11.92 cm and crosses the whole zone.
///
/// Derived — and verified in a unit test — as `bend_profile(STAGE_HEIGHT_M[k+1]) - bend_profile(STAGE_HEIGHT_M[k])`.
pub const STAGE_BEND_FRACTION: [f32; 6] = [0.0, 0.0, 0.057222, 0.942778, 0.0, 0.0];

/// Fraction of a body's apex deflection applied at stipe height `y` (metres, native scale).
///
/// Smoothstep, so it is `0` with zero slope below the zone (the lower stipe and volva stay planted and
/// unsheared) and `1` with zero slope above it (the cap translates rigidly and stays level). Duplicated in
/// `mycelia_fruit.wgsl`; see [`BEND_LO_M`].
pub fn bend_profile(y: f32) -> f32 {
    let u = ((y - BEND_LO_M) / (BEND_HI_M - BEND_LO_M)).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// The `growth` value past which the universal veil has ruptured and the cap is expanding. Below this the
/// egg is sealed; above it the mushroom is recognisably a mushroom. Used as the light gate (a primordium
/// only opens once seen) and as the amatoxin threshold — the toxin rides the gills and cap, and is nearly
/// absent from the volva (gills 13.38 > pileus 10.16 > stipe 9.99 >> volva 2.85 mg/g DM; Enjalbert et al.
/// 1999, 10.1016/s0764-4469(00)86651-2, tabulated by Vetter 2023, 10.3390/molecules28155932). Both of those
/// tissues appear only when the veil tears, so a body is only poisonous once it has a cap and gills.
pub const VEIL_RUPTURE_T: f32 = STAGE_T[3];

/// The autonomous-motion budget, in **world units per virtual second** (see the module header).
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

/// The vertex travel charged to segment `k`, metres at native scale: the morph's own chord, plus the share
/// of the stipe's bend laid down while `growth` crosses that segment, plus the sideways drift a tilted body
/// accumulates as it grows taller.
///
/// The three displacements need not point the same way, so their sum is an **upper bound** on the fastest
/// vertex's travel (triangle inequality). Bounding it is exactly what the speed limit needs.
fn segment_travel(k: usize, bend_m: f32, tilt: f32) -> f32 {
    STAGE_MAX_DISP[k]
        + STAGE_BEND_FRACTION[k] * bend_m.abs().min(MAX_BEND_M)
        + STAGE_HEIGHT_DELTA[k] * tilt.abs().min(MAX_TILT)
}

/// `d(growth)/dt` that holds the fastest-moving vertex at exactly `v_max`.
///
/// Within segment `k` the fastest vertex travels `segment_travel(k, bend) * body_scale` metres while
/// `growth` crosses `STAGE_T[k+1] - STAGE_T[k]`. Setting that vertex's speed to `v_max` and solving:
///
/// ```text
/// segment_duration = segment_travel(k, bend) * body_scale / v_max
/// dgrowth/dt       = (STAGE_T[k+1] - STAGE_T[k]) / segment_duration
/// ```
///
/// `bend_m` is the body's apex deflection in **native-scale metres** (see [`MAX_BEND_M`]); `tilt` is its
/// growth angle as a slope (see [`MAX_TILT`]). A bent or leaning mushroom therefore grows *slower* — which
/// is both what the eye requires and, pleasingly, what the stem is actually doing: the same growth resources
/// are being spent on curvature instead of extension (Moore 1991, 10.1111/j.1469-8137.1991.tb00940.x).
///
/// Always finite: every entry of [`STAGE_MAX_DISP`] is strictly positive, and `body_scale` is validated
/// `> 0`. The returned rate is unsigned — callers apply the biology gate (which may be negative, when a
/// primordium aborts or something takes a bite).
pub fn growth_rate(growth: f32, body_scale: f32, bend_m: f32, tilt: f32, v_max: f32) -> f32 {
    let k = segment_index(growth);
    let span = STAGE_T[k + 1] - STAGE_T[k];
    let duration = segment_travel(k, bend_m, tilt) * body_scale / v_max;
    span / duration
}

/// Virtual seconds for one body to go from sealed egg to adult at a fixed `v_max`, ignoring the rise.
/// Only used for diagnostics and tests — the live clock re-evaluates `v_max` every frame against the zoom.
pub fn egg_to_adult_secs(body_scale: f32, bend_m: f32, tilt: f32, v_max: f32) -> f32 {
    (0..6).map(|k| segment_travel(k, bend_m, tilt) * body_scale / v_max).sum()
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

// ── Caespitose flushes: bunches, and the colour they share ────────────────────────────────────────────
//
// Fruit bodies do not arrive one at a time. A flush erupts from a single aggregated hyphal knot, near
// synchronously, its members drawing on one translocated resource pool through the mycelial cords that feed
// the sink (Kües & Navarro-González 2015, Fungal Biol. Rev. 29:63, 10.1016/j.fbr.2015.05.001; cord-borne
// translocation to a resource sink: Wells & Boddy 1995, FEMS Microbiol. Ecol. 17:43,
// 10.1111/j.1574-6941.1995.tb00128.x). They are one genet, so they wear one pigment — with the spread in
// shade that mixed age and microclimate give any two caps on the same clump.
//
// `pin_min_spacing` used to enforce the opposite, and it was not wrong: neighbouring *knots* really do starve
// each other out. That competition is between genets. It is now `cluster_spacing`, and inside a cluster the
// only floor is geometry — two volvas cannot occupy the same ground.

/// Half-width of a cluster's Oklab `(a, b)` chroma offset, drawn per nucleus. Small: every cap must stay
/// inside the mat's grey-olive family, so this is the difference between "that clump is a little browner"
/// and "that clump is a different species".
pub const MAX_CLUSTER_AB: f32 = 0.020;

/// Half-width of the per-member offset around its cluster's colour. A quarter of the cluster spread, so a
/// bunch reads as one colour first and as individuals second.
pub const MAX_MEMBER_AB: f32 = 0.006;

/// Smallest centre-to-centre spacing two bodies of `body_scale` may have: their volvas touching. Below this
/// the sacs interpenetrate and the flush reads as one melted lump.
pub fn min_sibling_spacing(body_scale: f32) -> f32 {
    2.0 * VOLVA_RADIUS_M * body_scale
}

/// Deterministic layout of one caespitose flush: nucleus-relative offsets in world units, before any
/// wall-clearance seating. Element `0` is always the nucleus at the origin.
///
/// Size is drawn from `h²`, which skews toward the small flushes that dominate in the field: a pair or a
/// triple is common, an eight-body clump is not. Offsets are rejection-sampled in the annulus between
/// [`min_sibling_spacing`] and `cluster_radius`, so no two volvas overlap; a draw that cannot be placed in a
/// few attempts is simply dropped, which shrinks the flush rather than forcing a body into its sibling.
///
/// `cluster_radius` must exceed [`min_sibling_spacing`] — `validate_config` rejects a config where it does
/// not, because there would be no annulus to sample and every flush would silently collapse to its nucleus.
pub fn cluster_sites(seed: u32, body_scale: f32, cluster_radius: f32, size_max: u32) -> Vec<Vec2> {
    let r_min = min_sibling_spacing(body_scale);
    let ceiling = size_max.max(2);

    let h = hash01_u32(seed ^ 0x5127);
    let size = (2 + (h * h * (ceiling - 1) as f32) as u32).min(ceiling);

    let mut sites = vec![Vec2::ZERO];
    for m in 1..size {
        for attempt in 0..8u32 {
            let salt = seed ^ (0x9E00 + m * 16 + attempt);
            let angle = hash01_u32(salt) * std::f32::consts::TAU;
            let radius = r_min + hash01_u32(salt ^ 0xB3) * (cluster_radius - r_min);
            let p = Vec2::from_angle(angle) * radius;
            if sites.iter().all(|q| q.distance(p) >= r_min) {
                sites.push(p);
                break;
            }
        }
    }
    sites
}

/// A body's Oklab `(a, b)` offset: its cluster's colour, plus its own small deviation from it.
pub fn cap_ab_for(nucleus_seed: u32, member_seed: u32) -> Vec2 {
    let signed = |s: u32| 2.0 * hash01_u32(s) - 1.0;
    let cluster = Vec2::new(signed(nucleus_seed ^ 0xCA), signed(nucleus_seed ^ 0xCB)) * MAX_CLUSTER_AB;
    let member = Vec2::new(signed(member_seed ^ 0xD1), signed(member_seed ^ 0xD2)) * MAX_MEMBER_AB;
    cluster + member
}

// Oklab (Björn Ottosson, 2020). The perceptual space CSS Color 4 interpolates in, and the reason the cap's
// colour can vary without its *lightness* moving: `L` is what the cavity AO, the sheen and this LDR
// tonemapper were balanced against. Shift only `(a, b)` and the surface reads identically, in a new hue.
//
// **Duplicated in `mycelia_fruit.wgsl`**, which does the real work per fragment. These exist so the contract
// — round-trip fidelity, and that an `(a, b)` offset leaves `L` untouched — is provable in a unit test.

/// Linear sRGB → Oklab. `x` is `L`, `y` is `a`, `z` is `b`.
pub fn linear_srgb_to_oklab(c: Vec3) -> Vec3 {
    let l = 0.412_221_47 * c.x + 0.536_332_54 * c.y + 0.051_445_995 * c.z;
    let m = 0.211_903_5 * c.x + 0.680_699_5 * c.y + 0.107_396_96 * c.z;
    let s = 0.088_302_46 * c.x + 0.281_718_85 * c.y + 0.629_978_7 * c.z;
    // `cbrt` of a negative is defined and real, but a negative cone response is out of gamut; clamp so the
    // round trip is a function rather than a surprise.
    let (l_, m_, s_) = (l.max(0.0).cbrt(), m.max(0.0).cbrt(), s.max(0.0).cbrt());
    Vec3::new(
        0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
    )
}

/// Oklab → linear sRGB. May land outside `[0,1]` for an aggressive offset; the caller clamps.
pub fn oklab_to_linear_srgb(c: Vec3) -> Vec3 {
    let l_ = c.x + 0.396_337_78 * c.y + 0.215_803_76 * c.z;
    let m_ = c.x - 0.105_561_346 * c.y - 0.063_854_17 * c.z;
    let s_ = c.x - 0.089_484_18 * c.y - 1.291_485_5 * c.z;
    let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    Vec3::new(
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    )
}

#[cfg(test)]
#[path = "perceptual_tests.rs"]
mod tests;
