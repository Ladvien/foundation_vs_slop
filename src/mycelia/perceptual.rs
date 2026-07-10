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
        // Straight body and maximally bent body alike: the bend's travel is charged to the budget, so a
        // leaning mushroom must simply take longer, never move faster.
        for bend in [0.0, 0.5 * MAX_BEND_M, MAX_BEND_M] {
            for tilt in [0.0, MAX_TILT] {
                for steps in 0..=16u32 {
                    let viewport = MIN_ZOOM + (MAX_ZOOM - MIN_ZOOM) * (steps as f32 / 16.0);
                    let budget = v_max(THRESH, FOV, viewport);
                    for k in 0..6 {
                        // Sample strictly inside the segment so `segment_index` lands on `k`.
                        let g = STAGE_T[k] + 0.5 * (STAGE_T[k + 1] - STAGE_T[k]);
                        assert_eq!(segment_index(g), k, "sample fell outside segment {k}");

                        let rate = growth_rate(g, SHIPPED_SCALE, bend, tilt, budget);
                        let span = STAGE_T[k + 1] - STAGE_T[k];
                        // The worst vertex travels the morph chord PLUS its share of the bend PLUS the
                        // sideways drift of a tilted stem growing taller.
                        let travel = STAGE_MAX_DISP[k]
                            + STAGE_BEND_FRACTION[k] * bend
                            + STAGE_HEIGHT_DELTA[k] * tilt;
                        let vertex_speed = travel * SHIPPED_SCALE * rate / span;

                        assert!(
                            vertex_speed <= budget * (1.0 + 1e-4),
                            "segment {k}, bend {bend}, tilt {tilt}, viewport {viewport}: vertex \
                             {vertex_speed} m/s exceeds budget {budget} m/s",
                        );
                    }
                }
            }
        }
    }

    /// `STAGE_BEND_FRACTION` must be exactly what the profile does between consecutive stage heights, or the
    /// speed limit is budgeting for a different curve than the vertex shader draws.
    #[test]
    fn stage_bend_fraction_matches_the_profile() {
        let mut total = 0.0;
        for k in 0..6 {
            let expected = bend_profile(STAGE_HEIGHT_M[k + 1]) - bend_profile(STAGE_HEIGHT_M[k]);
            assert!(
                (STAGE_BEND_FRACTION[k] - expected).abs() < 1e-4,
                "segment {k}: constant {} vs profile {expected}",
                STAGE_BEND_FRACTION[k],
            );
            total += STAGE_BEND_FRACTION[k];
        }
        // The whole bend is laid down exactly once between the egg and the adult.
        assert!((total - 1.0).abs() < 1e-4, "bend fractions sum to {total}, want 1.0");
        // And almost all of it in segment 3, where the apex crosses the zone.
        assert!(STAGE_BEND_FRACTION[3] > 0.9);
    }

    /// The volva stays planted and the cap stays level: the profile is flat at both ends, so the lower stipe
    /// never shears and the pileus rides the bent stem rigidly (Moore 1991).
    #[test]
    fn bend_profile_is_flat_at_the_volva_and_at_the_cap() {
        assert_eq!(bend_profile(0.0), 0.0);
        assert_eq!(bend_profile(BEND_LO_M), 0.0);
        assert_eq!(bend_profile(EGG_HEIGHT_M), 0.0, "a sealed egg must be perfectly straight");
        assert!((bend_profile(BEND_HI_M) - 1.0).abs() < 1e-6);
        assert!((bend_profile(ADULT_HEIGHT_M) - 1.0).abs() < 1e-6, "the cap must translate rigidly");
        // The slope vanishes at both ends — that is what "planted" and "rigid" mean. A smoothstep leaves
        // the zone quadratically, so a step of 1/1000 of the zone must move the profile by ~3e-6, not 1e-3.
        let eps = 0.001 * (BEND_HI_M - BEND_LO_M);
        assert!(bend_profile(BEND_LO_M + eps) < 1e-5, "volva end is not flat");
        assert!(bend_profile(BEND_HI_M - eps) > 1.0 - 1e-5, "cap end is not flat");
        for i in 0..64 {
            let a = ADULT_HEIGHT_M * i as f32 / 64.0;
            let b = ADULT_HEIGHT_M * (i + 1) as f32 / 64.0;
            assert!(bend_profile(b) >= bend_profile(a) - 1e-6);
        }
    }

    /// A bent mushroom grows strictly slower than a straight one, and only in the segment that bends.
    #[test]
    fn bending_costs_time_only_where_the_stipe_curves() {
        let budget = v_max(THRESH, FOV, MIN_ZOOM);
        let straight = egg_to_adult_secs(SHIPPED_SCALE, 0.0, 0.0, budget);
        let bent = egg_to_adult_secs(SHIPPED_SCALE, MAX_BEND_M, 0.0, budget);
        assert!(bent > straight, "a bent stem must take longer: {bent} vs {straight}");

        // Segments 0, 1, 4, 5 lay down no bend, so their rate is untouched.
        for k in [0usize, 1, 4, 5] {
            let g = STAGE_T[k] + 0.5 * (STAGE_T[k + 1] - STAGE_T[k]);
            let a = growth_rate(g, SHIPPED_SCALE, 0.0, 0.0, budget);
            let b = growth_rate(g, SHIPPED_SCALE, MAX_BEND_M, 0.0, budget);
            assert!((a - b).abs() < 1e-6, "segment {k} should be unaffected by bend");
        }
        // Segment 3 carries 94% of it, so it slows markedly.
        let g3 = STAGE_T[3] + 0.5 * (STAGE_T[4] - STAGE_T[3]);
        assert!(
            growth_rate(g3, SHIPPED_SCALE, MAX_BEND_M, 0.0, budget)
                < 0.6 * growth_rate(g3, SHIPPED_SCALE, 0.0, 0.0, budget)
        );
    }

    /// The clearance design rests entirely on this: everything wide is high enough to be bent away, and the
    /// only thing that cannot be bent is the volva. If a future asset put a wide ring low on the stem, a
    /// bend could never clear it and the base nudge would have to grow to match.
    #[test]
    fn everything_wide_is_high_enough_to_bend_away() {
        let unbendable_max = RADIUS_PROFILE
            .iter()
            .enumerate()
            .filter(|(i, _)| bend_profile(radius_slice_height(*i)) < BENDABLE_MIN_PROFILE)
            .map(|(_, r)| *r)
            .fold(0.0f32, f32::max);
        assert!(
            (unbendable_max - VOLVA_RADIUS_M).abs() < 1e-3,
            "the widest unbendable ring should be the volva, got {unbendable_max}",
        );

        // ...and the cap, four times wider, sits where the profile has fully saturated.
        let cap_slices: Vec<usize> = RADIUS_PROFILE
            .iter()
            .enumerate()
            .filter(|(_, r)| **r > 0.05)
            .map(|(i, _)| i)
            .collect();
        assert!(!cap_slices.is_empty());
        for i in cap_slices {
            let p = bend_profile(radius_slice_height(i));
            assert!(p > 0.99, "cap slice {i} sits at profile {p}, a bend could not carry it clear");
        }
    }

    /// The cap overhangs the volva by 4x. That gap is the whole reason a mushroom whose base clears a wall
    /// can still push its cap through one, and the reason the fix is a bend rather than a keep-out radius.
    #[test]
    fn the_cap_overhangs_the_volva_far_enough_to_need_bending() {
        assert!(CAP_RADIUS_M > 2.0 * VOLVA_RADIUS_M);
        // A body planted with its volva just clearing a wall still overhangs by this much...
        let overhang = CAP_RADIUS_M - VOLVA_RADIUS_M;
        // ...and the bend ceiling must be able to carry the cap back out.
        assert!(MAX_BEND_M > overhang, "MAX_BEND_M {MAX_BEND_M} cannot clear an overhang of {overhang}");
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

        // At the shipped body_scale of 4.0: 0.1140 m x 4 = 45.6 cm of vertex travel, for a straight body.
        let secs = |viewport| egg_to_adult_secs(SHIPPED_SCALE, 0.0, 0.0, v_max(THRESH, FOV, viewport));
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
            growth_rate(g, SHIPPED_SCALE, 0.0, 0.0, budget)
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
        for i in 0..=100u32 {
            let k = segment_index(i as f32 / 100.0);
            assert!(k < 6);
        }
        // Exact stage boundaries belong to the segment they close.
        assert_eq!(segment_index(STAGE_T[1]), 0);
        assert_eq!(segment_index(STAGE_T[1] + 1e-6), 1);
    }

    /// **The other invariant.** No albedo or glow transition may complete faster than the slow-change
    /// window, at any frame rate. Stepped at 60 Hz from either end, `slew` must need at least
    /// `MIN_APPEARANCE_RAMP_SECS` to cross the full `[0,1]` range.
    #[test]
    fn slew_never_completes_faster_than_the_slow_change_window() {
        for (from, to) in [(0.0f32, 1.0f32), (1.0, 0.0)] {
            for hz in [30.0f32, 60.0, 144.0] {
                let dt = 1.0 / hz;
                let (mut v, mut elapsed) = (from, 0.0f32);
                while (v - to).abs() > 1e-6 && elapsed < 60.0 {
                    v = slew(v, to, dt, MIN_APPEARANCE_RAMP_SECS);
                    elapsed += dt;
                }
                assert!(
                    elapsed >= MIN_APPEARANCE_RAMP_SECS - dt,
                    "{from} → {to} at {hz} Hz completed in {elapsed}s, faster than the \
                     {MIN_APPEARANCE_RAMP_SECS}s window",
                );
            }
        }
    }

    /// A paused clock freezes the signal rather than snapping it to the target — the mold holds its
    /// shading exactly where it was. And `slew` never overshoots, so it cannot ring around the target.
    #[test]
    fn slew_is_a_no_op_at_zero_dt_and_never_overshoots() {
        assert_eq!(slew(0.3, 1.0, 0.0, MIN_APPEARANCE_RAMP_SECS), 0.3);
        // A `dt` far larger than the whole ramp lands exactly on the target, never past it.
        assert_eq!(slew(0.0, 1.0, 1e6, MIN_APPEARANCE_RAMP_SECS), 1.0);
        assert_eq!(slew(1.0, 0.0, 1e6, MIN_APPEARANCE_RAMP_SECS), 0.0);
        // Already there: a no-op regardless of `dt`.
        assert_eq!(slew(0.5, 0.5, 0.016, MIN_APPEARANCE_RAMP_SECS), 0.5);
    }

    /// Monotone in the direction of travel, and it reproduces the fruit body's tint limiter exactly — the
    /// idiom `fruit::grow_fruit_bodies` used before this function existed.
    #[test]
    fn slew_matches_the_open_coded_tint_limiter() {
        let (dt, ramp) = (1.0 / 60.0, MIN_APPEARANCE_RAMP_SECS);
        let (mut a, mut b) = (0.0f32, 0.0f32);
        for i in 0..600 {
            let target = i as f32 / 600.0;
            a = slew(a, target, dt, ramp);
            // The original two-liner, verbatim.
            let step = dt / ramp;
            b += (target - b).clamp(-step, step);
            assert!((a - b).abs() < 1e-9, "step {i}: {a} vs {b}");
        }
    }

    const SHIPPED_CLUSTER_RADIUS: f32 = 0.7;
    const SHIPPED_SIZE_MAX: u32 = 8;

    /// **The flush invariant.** No two bodies in a bunch may stand closer than their volvas touching, and
    /// none may stray outside the cluster radius. Both hold for every seed, because the layout is
    /// rejection-sampled rather than nudged into place.
    #[test]
    fn a_flush_never_overlaps_its_own_volvas_nor_leaves_its_radius() {
        let r_min = min_sibling_spacing(SHIPPED_SCALE);
        assert!(SHIPPED_CLUSTER_RADIUS > r_min, "the shipped radius must leave an annulus to sample");
        for seed in 0..400u32 {
            let sites = cluster_sites(seed, SHIPPED_SCALE, SHIPPED_CLUSTER_RADIUS, SHIPPED_SIZE_MAX);
            assert!((2..=SHIPPED_SIZE_MAX as usize).contains(&sites.len()), "seed {seed}: {sites:?}");
            assert_eq!(sites[0], Vec2::ZERO, "member 0 is the nucleus");
            for (i, a) in sites.iter().enumerate() {
                assert!(
                    a.length() <= SHIPPED_CLUSTER_RADIUS + 1e-5,
                    "seed {seed}: member {i} at {a:?} left the cluster radius",
                );
                for b in sites.iter().skip(i + 1) {
                    assert!(
                        a.distance(*b) >= r_min - 1e-5,
                        "seed {seed}: volvas overlap, {a:?} and {b:?} are {} apart (min {r_min})",
                        a.distance(*b),
                    );
                }
            }
        }
    }

    /// A flush is a deterministic function of its nucleus's seed — the pin order must not depend on when a
    /// readback happened to land. And the size distribution skews small, as real flushes do.
    #[test]
    fn flush_layout_is_deterministic_and_skews_small() {
        for seed in [0u32, 1, 7, 4242, u32::MAX] {
            let a = cluster_sites(seed, SHIPPED_SCALE, SHIPPED_CLUSTER_RADIUS, SHIPPED_SIZE_MAX);
            let b = cluster_sites(seed, SHIPPED_SCALE, SHIPPED_CLUSTER_RADIUS, SHIPPED_SIZE_MAX);
            assert_eq!(a, b, "seed {seed} laid out two different flushes");
        }
        let sizes: Vec<usize> = (0..500u32)
            .map(|s| cluster_sites(s, SHIPPED_SCALE, SHIPPED_CLUSTER_RADIUS, SHIPPED_SIZE_MAX).len())
            .collect();
        let small = sizes.iter().filter(|n| **n <= 4).count();
        assert!(small * 2 > sizes.len(), "most flushes should be small, got {small}/{}", sizes.len());
    }

    /// The whole reason the cap's colour lives in Oklab: an `(a, b)` offset must leave **lightness exactly
    /// alone**. `L` is what the AO, the sheen and the tonemapper were balanced against — a hue that also
    /// moved `L` would relight the mushroom.
    #[test]
    fn an_oklab_chroma_offset_never_moves_lightness() {
        let cap_young = Vec3::new(0.444, 0.450, 0.417);
        let cap_old = Vec3::new(0.135, 0.155, 0.128);
        for base in [cap_young, cap_old] {
            let lab = linear_srgb_to_oklab(base);
            for seed in 0..200u32 {
                let ab = cap_ab_for(seed, seed ^ 0xF00D);
                let shifted = oklab_to_linear_srgb(Vec3::new(lab.x, lab.y + ab.x, lab.z + ab.y));
                let back = linear_srgb_to_oklab(shifted);
                assert!(
                    (back.x - lab.x).abs() < 1e-4,
                    "seed {seed}: lightness moved {} -> {}",
                    lab.x,
                    back.x,
                );
                assert!(shifted.min_element() >= -1e-3, "seed {seed} left the gamut: {shifted:?}");
            }
        }
    }

    /// Oklab round-trips. If this drifts, the shader's duplicate of these matrices is describing a different
    /// colour space from the one the tests above vouch for.
    #[test]
    fn oklab_round_trips() {
        let probes = [
            Vec3::new(0.048, 0.059, 0.051),
            Vec3::new(0.238, 0.396, 0.323),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(0.5, 0.25, 0.75),
            Vec3::ZERO,
        ];
        for c in probes {
            let back = oklab_to_linear_srgb(linear_srgb_to_oklab(c));
            assert!((back - c).abs().max_element() < 1e-4, "{c:?} round-tripped to {back:?}");
        }
    }

    /// A bunch reads as one colour: every member sits within `MAX_MEMBER_AB` of its cluster's shade, and no
    /// body ever leaves the mat's family.
    #[test]
    fn cluster_members_share_a_colour_and_stay_in_the_family() {
        for nucleus in 0..100u32 {
            let members: Vec<Vec2> = (0..8).map(|m| cap_ab_for(nucleus, nucleus ^ (0xF000 + m))).collect();
            for ab in &members {
                assert!(
                    ab.length() <= MAX_CLUSTER_AB * std::f32::consts::SQRT_2
                        + MAX_MEMBER_AB * std::f32::consts::SQRT_2
                        + 1e-6,
                    "nucleus {nucleus}: {ab:?} strayed outside the family",
                );
            }
            // Members of one cluster differ from each other by at most twice the member spread.
            for (i, a) in members.iter().enumerate() {
                for b in members.iter().skip(i + 1) {
                    assert!(
                        a.distance(*b) <= 2.0 * MAX_MEMBER_AB * std::f32::consts::SQRT_2 + 1e-6,
                        "nucleus {nucleus}: siblings {a:?} and {b:?} do not share a colour",
                    );
                }
            }
        }
    }

    /// `f32::clamp` **propagates** NaN — it does not return the min, as an earlier comment here claimed.
    ///
    /// So a NaN `growth` leaves `g` NaN, every `g <= STAGE_T[k + 1]` comparison is false, `find` yields
    /// nothing, and `unwrap_or(5)` saturates to the **high** end. The index stays in range, which is all
    /// `segment_index` promises — but the weights built from it do not, and glTF morph weights of NaN collapse
    /// the mesh. Nothing downstream may rely on this being absorbed: `fruit::drive_morph_weights` rejects a
    /// non-finite `growth` outright.
    #[test]
    fn nan_growth_saturates_the_index_but_poisons_the_weights() {
        assert!(f32::NAN.clamp(0.0, 1.0).is_nan(), "clamp must propagate NaN, not absorb it");
        assert_eq!(segment_index(f32::NAN), 5);
        assert!(
            stage_weights(f32::NAN).iter().any(|w| w.is_nan()),
            "a NaN growth must be caught upstream, because it is not caught here"
        );
    }

    /// Every finite `growth`, in range or out of it, yields six finite weights.
    #[test]
    fn stage_weights_are_finite_over_the_finite_domain() {
        let probes = [-1e9, -1.0, -1e-6, 0.0, 0.5, 1.0, 1.0 + 1e-6, 1e9, f32::MIN, f32::MAX];
        for g in probes {
            let w = stage_weights(g);
            assert!(w.iter().all(|x| x.is_finite()), "growth {g} produced {w:?}");
            assert!(w.iter().all(|x| (0.0..=1.0).contains(x)), "growth {g} produced {w:?}");
        }
    }
}
