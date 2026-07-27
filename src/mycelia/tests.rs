//! Tests for the mycelia module — moved out of `mod.rs` so the implementation reads without
//! scrolling past them. Still a child module of the same parent, so `use super::*` resolves exactly
//! as before; a pure move.

use super::*;

/// A known-good config, matching the shipped `mycelia:` slice.
/// Visible to the sibling submodules' tests (e.g. `habitat`), which need a valid config to build against.
pub(super) fn valid() -> MyceliaConfig {
    MyceliaConfig {
        field_size: 1024,
        sim_hz: 1.5,
        warmup_ticks: 200,
        agent_count: 55_000,
        sense_angle: 0.40,
        sense_dist: 9.0,
        rotate_angle: 0.50,
        step_size: 1.0,
        deposit_amount: 1.0,
        diffuse_weight: 0.18,
        decay: 0.96,
        trail_max: 24.0,
        dt: 1.0,
        feed: 0.036,
        kill: 0.060,
        kill_barren: 0.09,
        d_u: 0.16,
        d_v: 0.08,
        bloom_seed: 0.06,
        habitat_coverage: 0.25,
        patch_spacing: 4.0,
        patch_radius_min: 2.0,
        patch_radius_max: 5.0,
        corridor_infest_chance: 0.12,
        edge_noise_amp: 0.35,
        edge_noise_scale: 0.15,
        agent_hab_min: 0.02,
        damp_weights: vec![
            DampWeight { tag: "bathroom".into(), weight: 3.0 },
            DampWeight { tag: "kitchen".into(), weight: 2.5 },
            DampWeight { tag: "hall".into(), weight: 1.2 },
            DampWeight { tag: "living".into(), weight: 1.0 },
            DampWeight { tag: "bedroom".into(), weight: 0.6 },
            DampWeight { tag: "office".into(), weight: 0.4 },
        ],
        photophobia: 9.0,
        chemo_gain: 6.0,
        disturbance_gain: 5.0,
        wall_repel: 12.0,
        wall_affinity: 5.0,
        carrion_bloom: 0.30,
        wall_reach: 0.6,
        hab_rate: 0.35,
        hab_recover: 0.08,
        hab_strength: 0.75,
        glow_gain: 1.35,
        intensity: 1.0,
        vein_lo: 3.0,
        vein_hi: 12.0,
        normal_strength: 1.1,
        wet_roughness: 0.42,
        climb_height: 0.85,
        fiber_scale: 8.0,
        fiber_strength: 1.6,
        margin_roughness: 0.55,
        sheen_strength: 0.18,
        ao_strength: 0.75,
        reveal_warp_amp: 0.012,
        reveal_warp_scale: 0.7,
        v_fruit: 0.35,
        u_exhausted: 0.30,
        pin_dwell_secs: 6.0,
        cluster_spacing: 3.0,
        cluster_radius: 0.7,
        cluster_size_max: 8,
        max_fruit_bodies: 40,
        body_scale: 4.0,
        maintain_v: 0.20,
        motion_threshold_deg_per_s: 0.02,
        screen_fov_deg_v: 30.0,
        intent_focus_count: 3,
        intent_focus_radius: 6.0,
        intent_speed_scale: 40.0,
        intent_roam_period: 45.0,
        species: vec![species::death_cap_config_row()],
    }
}

#[test]
fn shipped_defaults_validate() {
    assert!(validate_config(&valid()).is_ok());
}

/// `field_size` must tile the 8×8 workgroup exactly, or the 2D dispatch misses texels.
#[test]
fn field_size_must_tile_the_workgroup() {
    let mut c = valid();
    c.field_size = 1020; // not a multiple of 8 (1020 / 8 = 127.5)
    assert!(validate_config(&c).is_err());
    c.field_size = 0;
    assert!(validate_config(&c).is_err());
    c.field_size = 8192; // over the allocation cap
    assert!(validate_config(&c).is_err());
}

/// `decay >= 1` never fades (the trail floods to `trail_max` everywhere and the network dissolves);
/// `decay <= 0` erases it every tick. Both are degenerate, so both must be rejected loudly.
#[test]
fn decay_must_be_strictly_between_zero_and_one() {
    for bad in [0.0, 1.0, 1.5, -0.1] {
        let mut c = valid();
        c.decay = bad;
        assert!(validate_config(&c).is_err(), "decay={bad} should be rejected");
    }
}

/// Gray-Scott only forms patterns with *unequal* diffusion: `d_v >= d_u` kills the Turing instability.
#[test]
fn biomass_must_diffuse_slower_than_substrate() {
    let mut c = valid();
    c.d_v = c.d_u;
    assert!(validate_config(&c).is_err());
    c.d_v = c.d_u + 0.01;
    assert!(validate_config(&c).is_err());
}

/// Climbing past the top of a wall is meaningless, and `wet_roughness` outside Bevy's clamp range is a
/// config mistake rather than an intent.
#[test]
fn surface_dials_are_bounded_by_physical_reality() {
    let mut c = valid();
    c.climb_height = crate::dungeon::WALL_HEIGHT + 0.1;
    assert!(validate_config(&c).is_err());

    for bad in [0.0, 0.05, 1.5] {
        let mut c = valid();
        c.wet_roughness = bad;
        assert!(validate_config(&c).is_err(), "wet_roughness={bad} should be rejected");
    }

    let mut c = valid();
    c.wall_reach = 0.0; // a zero reach would divide by zero in the falloff
    assert!(validate_config(&c).is_err());
}

/// An inverted vein window would make `smoothstep` degenerate.
#[test]
fn vein_window_must_be_ordered() {
    let mut c = valid();
    c.vein_hi = c.vein_lo;
    assert!(validate_config(&c).is_err());
}

/// Unit-range dials are rejected outside `0..=1` rather than silently clamped.
#[test]
fn unit_range_dials_are_not_clamped() {
    for bad in [-0.1, 1.1] {
        let mut c = valid();
        c.intensity = bad;
        assert!(validate_config(&c).is_err(), "intensity={bad} should be rejected");

        let mut c = valid();
        c.diffuse_weight = bad;
        assert!(validate_config(&c).is_err(), "diffuse_weight={bad} should be rejected");

        let mut c = valid();
        c.hab_strength = bad;
        assert!(validate_config(&c).is_err(), "hab_strength={bad} should be rejected");
    }
}

/// NaN must not sneak past the comparisons (`v > 0.0` is false for NaN, but be explicit about it).
#[test]
fn nan_is_rejected() {
    let mut c = valid();
    c.sense_dist = f32::NAN;
    assert!(validate_config(&c).is_err());
}

/// A body pins at `v_fruit` and reabsorbs below `maintain_v`. Crossed, every primordium would begin
/// aborting on the frame it committed and the mold would flicker mushrooms rather than grow them.
#[test]
fn maintenance_threshold_must_sit_below_the_fruiting_threshold() {
    let mut c = valid();
    c.maintain_v = c.v_fruit;
    assert!(validate_config(&c).is_err());
    c.maintain_v = c.v_fruit + 0.1;
    assert!(validate_config(&c).is_err());
}

/// The pin condition is a conjunction — thick mat AND spent substrate. Thresholds that cannot both hold
/// at once mean nothing ever fruits, which would look exactly like a bug in the scan pass.
#[test]
fn fruiting_thresholds_must_be_jointly_satisfiable() {
    let mut c = valid();
    c.v_fruit = 0.8;
    c.u_exhausted = 0.5; // 1.3 > 1.0: no texel can hold V > 0.8 while U < 0.5
    assert!(validate_config(&c).is_err());
}

/// The perception budget divides by `screen_fov_deg_v` and scales by `motion_threshold_deg_per_s`; a
/// zero or absurd value silently produces an infinite growth rate rather than a visibly wrong one.
#[test]
fn perception_budget_is_bounded_by_psychophysics() {
    for bad in [0.0, -0.02] {
        let mut c = valid();
        c.motion_threshold_deg_per_s = bad;
        assert!(validate_config(&c).is_err(), "threshold={bad} should be rejected");
    }
    // 20 deg/s is a briskly moving object, not a subliminal one. Catch the misplaced decimal.
    let mut c = valid();
    c.motion_threshold_deg_per_s = 20.0;
    assert!(validate_config(&c).is_err());

    for bad in [0.0, 0.5, 200.0] {
        let mut c = valid();
        c.screen_fov_deg_v = bad;
        assert!(validate_config(&c).is_err(), "fov={bad} should be rejected");
    }
}

/// The fruiting dials are rejected outside their physical ranges rather than clamped.
#[test]
fn fruiting_dials_are_bounded() {
    let mut c = valid();
    c.max_fruit_bodies = 0;
    assert!(validate_config(&c).is_err());

    for bad in [0.0, -1.0] {
        let mut c = valid();
        c.cluster_spacing = bad;
        assert!(validate_config(&c).is_err(), "cluster_spacing={bad} should be rejected");

        let mut c = valid();
        c.body_scale = bad;
        assert!(validate_config(&c).is_err(), "body_scale={bad} should be rejected");

        let mut c = valid();
        c.pin_dwell_secs = bad;
        assert!(validate_config(&c).is_err(), "pin_dwell_secs={bad} should be rejected");
    }

    for bad in [-0.1, 1.1] {
        let mut c = valid();
        c.v_fruit = bad;
        assert!(validate_config(&c).is_err(), "v_fruit={bad} should be rejected");
    }
}
