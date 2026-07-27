//! Tests for `perceptual.rs` — moved out of the module file so the implementation reads without
//! scrolling past them. Still a child module of the same parent, so `use super::*` resolves
//! exactly as before; this is a pure move.

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
