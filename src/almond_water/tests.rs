//! Tests for `mod.rs` — moved out of the module file so the implementation reads without
//! scrolling past them. Still a child module of the same parent, so `use super::*` resolves
//! exactly as before; this is a pure move.

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
        almond_tint: [0.20, 0.80, 0.85],
        min_visible_level: 1.0,
        film_thickness_nm: 320.0,
        film_ior: 1.33,
        iridescence_strength: 0.25,
        moisture_feed_gain: 0.3,
        iridescence_mute: 0.6,
        poison_tint: [0.55, 0.95, 0.15],
        base_alpha: 0.85,
        rim_strength: 1.6,
        glint_strength: 0.6,
        ripple_strength: 1.0,
        edge_feather: 0.22,
        feather_scale: 0.9,
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
