//! Guide-strand solver — **Dynamic Follow-The-Leader** (Müller, Kim & Chentanez, "Fast Simulation of
//! Inextensible Hair and Fur", VRIPHYS 2012), with an XPBD skip-1 bend constraint for anti-curl.
//!
//! # Why DFTL replaced the Verlet + XPBD-distance predictor this module used to run
//!
//! The old solver enforced segment length with an XPBD distance constraint, which *converges toward*
//! the rest length over iterations. FTL **projects** each particle onto the sphere of radius `l₀`
//! around its already-corrected predecessor in a single root→tip pass, so every segment is exactly
//! `l₀` afterwards — no iteration count to tune, no stretch under load. That is the property the old
//! `a_pinned_clump_settles_without_stretching_past_rest_length` test could only assert to within 15%
//! and this one asserts to 1e-5.
//!
//! FTL alone is not usable, because it only ever moves the *follower*: it injects momentum the leader
//! never paid for, and a chain snapped taut visibly gains energy. The paper's contribution is the
//! velocity correction (Eq. 9),
//!
//! ```text
//! v_i = (p_i − p_i^old)/Δt  −  s · d_{i+1}/Δt
//! ```
//!
//! where `d_{i+1}` is the *successor's* FTL position correction and `s ∈ [0,1]` damps it (the paper
//! illustrates `s = 0.9`). Subtracting the successor's correction from the leader's velocity cancels
//! the artifact of FTL's implicit uneven mass distribution (m, sm, s²m…). [`dftl_velocities`] is that
//! equation and nothing else.
//!
//! Substepping follows Macklin, Storey, Lu, Terdiman, Chentanez, Jeschke & Müller, "Small Steps in
//! Physics Simulation", MIG 2019, DOI 10.1145/3309486.3340247 — a few substeps with one relaxation
//! pass each beats few substeps with many iterations, for both stability and cost. The bend
//! constraint is XPBD-compliant (Müller, Macklin, Chentanez, Jeschke & Kim, "Detailed Rigid Body
//! Simulation with Extended Position Based Dynamics", CGF 2020, DOI 10.1111/cgf.14105) because
//! compliance decouples perceived stiffness from timestep, which matters running on variable-dt
//! `Update`/`PostUpdate` rather than a fixed tick.
//!
//! # What DFTL does *not* fix, and why [`is_teleport`] exists
//!
//! A root that jumps 80 m in one frame — which the world rebuild on `OnEnter(RunState::Active)` does
//! every single run — gives `d₁ ≈ 80 m`. Eq. 9 with `s = 0.9` still leaves roughly `0.1 × 80/Δt`, so
//! the chain would be *exactly the right length* and *travelling at several hundred m/s*. That is the
//! same artifact the old solver produced, arrived at by a different route. The honest fix is to
//! detect the discontinuity and reseed, which is what [`is_teleport`] + [`reseed`] do — one recovery
//! path, shared with first-time seeding, so a diverged guide recovers by the same code a fresh one
//! starts on.
//!
//! Everything in this module is pure math over slices: no ECS types, no queries, no `Assets`. That is
//! deliberate — it is what lets the whole solver run in the GPU-free `cargo test` hard gate.

use bevy::prelude::*;

use super::bind::RootBind;
use super::HairSettings;

/// One simulated guide strand. `pos[0]` is the root, hard-pinned to the host surface every frame.
pub(super) struct Guide {
    pub pos: Vec<Vec3>,
    pub vel: Vec<Vec3>,
    /// Scratch: this substep's FTL correction per particle — `d_i` in Müller et al. 2012 Eq. 9.
    /// Lives on the guide rather than in a local so the solver never allocates per frame.
    corr: Vec<Vec3>,
    /// How this guide's root is attached to the host surface, baked once at spawn. See
    /// [`super::bind`] — the root position and growth direction both come from evaluating this against
    /// the live joint palette, never from a single bone transform.
    pub bind: RootBind,
    /// Per-guide wind-phase offset (radians) so guides don't sway in lockstep.
    pub phase: f32,
}

impl Guide {
    pub fn new(segments: usize, bind: RootBind, phase: f32) -> Self {
        let particles = segments + 1;
        Guide {
            pos: vec![Vec3::ZERO; particles],
            vel: vec![Vec3::ZERO; particles],
            corr: vec![Vec3::ZERO; particles],
            bind,
            phase,
        }
    }

    /// Total rest length of the strand, root to tip.
    #[inline]
    pub fn span(&self, rest_length: f32) -> f32 {
        rest_length * (self.pos.len().saturating_sub(1)) as f32
    }
}

/// Did the root **move**, or was it **teleported**?
///
/// A root that travels further in one frame than the whole strand is long cannot be followed: DFTL
/// would produce a correctly-inextensible chain moving at several hundred m/s, which is the stretched
/// spike artifact by another name. Non-finite input counts as a teleport too, so one guard covers both
/// the world-rebuild jump and any NaN that reaches the root.
#[inline]
pub(super) fn is_teleport(prev_root: Vec3, root: Vec3, span: f32) -> bool {
    !prev_root.is_finite() || !root.is_finite() || prev_root.distance_squared(root) > span * span
}

/// Lay a guide straight along `grow_dir` from `root`, at rest, with zero velocity.
///
/// This is both the first-time seed and the teleport recovery — deliberately one function, so a
/// diverged guide recovers by exactly the path a fresh one starts on rather than through a second,
/// less-tested branch.
pub(super) fn reseed(g: &mut Guide, root: Vec3, grow_dir: Vec3, rest_length: f32) {
    let dir = grow_dir.normalize_or_zero();
    // A zero growth direction would collapse every particle onto the root, which FTL cannot recover
    // from (it has no direction to project along). Fall back to straight down — the direction gravity
    // would have pulled the strand to anyway.
    let dir = if dir == Vec3::ZERO { Vec3::NEG_Y } else { dir };
    for i in 0..g.pos.len() {
        g.pos[i] = root + dir * (i as f32 * rest_length);
        g.vel[i] = Vec3::ZERO;
        g.corr[i] = Vec3::ZERO;
    }
}

/// Follow-The-Leader inextensibility, root→tip, one pass.
///
/// Projects each particle onto the sphere of radius `l0` centred on its **already-corrected**
/// predecessor. `corr[i]` records the displacement applied to particle `i`, which [`dftl_velocities`]
/// then subtracts from particle `i-1`'s velocity per Eq. 9.
pub(super) fn ftl_pass(pos: &mut [Vec3], corr: &mut [Vec3], l0: f32) {
    corr[0] = Vec3::ZERO;
    for i in 1..pos.len() {
        let d = pos[i] - pos[i - 1];
        let len = d.length();
        // A degenerate separation has no direction to project along. Hold the particle where it is
        // rather than divide by ~0 — the same guard the superseded `xpbd_distance_correct` carried.
        let want = if len < 1.0e-6 { pos[i] } else { pos[i - 1] + (d / len) * l0 };
        corr[i] = want - pos[i];
        pos[i] = want;
    }
}

/// DFTL Eq. 9: `v_i = (p_i − p_i^old)/Δt − s·d_{i+1}/Δt`.
///
/// The tip has no successor, so its correction term is zero. The root is pinned and always has zero
/// velocity.
pub(super) fn dftl_velocities(pos: &[Vec3], old: &[Vec3], corr: &[Vec3], vel: &mut [Vec3], dt: f32, s: f32) {
    let inv_dt = 1.0 / dt;
    for i in 1..pos.len() {
        let successor = corr.get(i + 1).copied().unwrap_or(Vec3::ZERO);
        vel[i] = ((pos[i] - old[i]) - s * successor) * inv_dt;
    }
    vel[0] = Vec3::ZERO;
}

/// Single XPBD relaxation pass for one distance constraint between `pos[i]` and `pos[j]` (Müller et
/// al. 2020, eq. 4-6, single-iteration-per-substep form: the Lagrange multiplier resets to 0 each
/// call, so `Δλ = -C / (w_i + w_j + α̃)`). Only the bend constraint uses this now — FTL owns length.
fn xpbd_distance_correct(pos: &mut [Vec3], i: usize, j: usize, rest: f32, alpha_tilde: f32) {
    let d = pos[j] - pos[i];
    let len = d.length();
    if len < 1.0e-6 {
        return; // degenerate separation — skip rather than divide by ~0
    }
    let dir = d / len;
    let c = len - rest;
    // Particle 0 is pinned (inverse mass 0); every other particle has unit inverse mass.
    let (w_i, w_j) = (if i == 0 { 0.0 } else { 1.0 }, if j == 0 { 0.0 } else { 1.0 });
    let w_sum = w_i + w_j;
    if w_sum <= 0.0 {
        return; // both ends pinned, nothing to correct
    }
    let d_lambda = -c / (w_sum + alpha_tilde);
    pos[i] -= w_i * d_lambda * dir;
    pos[j] += w_j * d_lambda * dir;
}

/// Skip-1 anti-curl pass. Applied *before* FTL on purpose: bend biases direction, FTL owns length, so
/// the ordering leaves inextensibility exact regardless of how the bend constraint is tuned.
fn bend_pass(pos: &mut [Vec3], rest_length: f32, alpha_tilde: f32) {
    let n = pos.len();
    if n < 3 {
        return;
    }
    for i in 0..n - 2 {
        xpbd_distance_correct(pos, i, i + 2, 2.0 * rest_length, alpha_tilde);
    }
}

/// Hand-rolled layered-sine ambient wind (no RNG crate, matching project convention) — a CPU force,
/// not sampled from the shader-side `assets/shaders/noise.wgsl` library (that library is
/// fragment/GPU-side; this is a Rust force calculation, so a shader import doesn't apply here). Tip
/// particles sway more than root particles since the tip is the free end.
pub(super) fn wind_accel(phase: f32, particle_idx: usize, elapsed: f32, s: &HairSettings) -> Vec3 {
    let w = elapsed * s.wind_freq + phase;
    let sway = (w.sin() + 0.4 * (w * 2.3 + phase).sin()) * s.wind_strength;
    let tip_factor = (particle_idx as f32 / 4.0).min(1.0);
    Vec3::new(sway, 0.15 * sway, sway * 0.7 * (w * 0.6).cos()) * tip_factor
}

/// Advance one guide by one frame.
///
/// `root` is this frame's world-space attachment point and `grow_dir` its world-space growth
/// direction; `scratch` is a caller-owned buffer reused across every guide so the solver never
/// allocates. Returns `true` if the guide was reseeded rather than integrated.
pub(super) fn step_guide(
    g: &mut Guide,
    root: Vec3,
    grow_dir: Vec3,
    dt: f32,
    elapsed: f32,
    cfg: &HairSettings,
    scratch: &mut Vec<Vec3>,
) -> bool {
    let n = g.pos.len();
    if n < 2 || !(dt > 0.0) || !dt.is_finite() {
        return false;
    }

    // The discontinuity guard comes first: everything below assumes the root moved by an amount the
    // chain could plausibly follow.
    if is_teleport(g.pos[0], root, g.span(cfg.rest_length)) {
        reseed(g, root, grow_dir, cfg.rest_length);
        return true;
    }

    let substeps = cfg.substeps.max(1);
    let h = dt / substeps as f32;
    let alpha_bend = cfg.bend_compliance / (h * h);
    // `damping` is the fraction of velocity RETAINED PER SECOND, so 30 fps and 240 fps settle
    // identically — the same frame-rate-independent exponential the animation layer uses for its
    // weight ease (`anim::FADE_TAU`, Holmér, "Lerp smoothing is broken", 2023). The superseded Verlet
    // solver applied its damping factor once per *substep*, which silently changed the look whenever
    // `substeps` moved.
    let retain = cfg.damping.clamp(0.0, 1.0).powf(h);
    let gravity_accel = Vec3::NEG_Y * cfg.gravity * cfg.gravity_scale;

    scratch.clear();
    scratch.extend_from_slice(&g.pos);

    for _ in 0..substeps {
        // `p_i^old` for Eq. 9 is the position at the START of this substep.
        scratch.copy_from_slice(&g.pos);

        // 1) Predict (symplectic Euler). The root is pinned and never integrated.
        for i in 1..n {
            let accel = gravity_accel + wind_accel(g.phase, i, elapsed, cfg);
            g.vel[i] = g.vel[i] * retain + accel * h;
            g.pos[i] += g.vel[i] * h;
        }

        // 2) Bend, then pin, then length. Order matters — see `bend_pass`.
        bend_pass(&mut g.pos, cfg.rest_length, alpha_bend);
        g.pos[0] = root;
        ftl_pass(&mut g.pos, &mut g.corr, cfg.rest_length);

        // 3) Velocities, with the DFTL correction.
        dftl_velocities(&g.pos, scratch, &g.corr, &mut g.vel, h, cfg.ftl_correction);

        // 4) A last-resort speed ceiling. Not a second solver path — the solver above is the only one
        //    that runs — but a numeric backstop so a pathological input (a host mesh that jumps just
        //    under the teleport threshold every frame, say) degrades into slow hair rather than a
        //    chain whipping across the level.
        for v in g.vel.iter_mut().skip(1) {
            *v = v.clamp_length_max(cfg.max_speed);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> HairSettings {
        HairSettings {
            clumps_per_unit: 1,
            segments_per_strand: 5,
            rest_length: 0.05,
            bend_compliance: 0.002,
            damping: 0.05,
            ftl_correction: 0.9,
            gravity: 9.8,
            gravity_scale: 0.6,
            wind_strength: 0.0,
            wind_freq: 1.6,
            substeps: 4,
            max_speed: 12.0,
            strand_width_root: 0.062,
            strand_width_tip: 0.010,
            tint: [0.25, 0.14, 0.08],
        }
    }

    /// A root rigidly bound to one joint at full weight — the Valkyrie scalp-cap case, and the one
    /// `bind::tests::eval_root_follows_a_single_rigid_joint_exactly` proves is exact.
    fn test_bind() -> RootBind {
        RootBind { rest: Vec3::ZERO, normal: Vec3::NEG_Y, slots: [0; 4], weights: [1.0, 0.0, 0.0, 0.0] }
    }

    fn settled(cfg: &HairSettings, frames: usize, root: Vec3) -> Guide {
        let mut g = Guide::new(cfg.segments_per_strand, test_bind(), 0.0);
        reseed(&mut g, root, Vec3::NEG_Y, cfg.rest_length);
        let mut scratch = Vec::new();
        for _ in 0..frames {
            step_guide(&mut g, root, Vec3::NEG_Y, 1.0 / 60.0, 0.0, cfg, &mut scratch);
        }
        g
    }

    /// The property XPBD could only converge toward. One pass, from an arbitrarily tangled start.
    #[test]
    fn ftl_pass_makes_every_segment_exactly_the_rest_length() {
        let l0 = 0.05;
        let mut pos = vec![
            Vec3::ZERO,
            Vec3::new(3.0, -1.0, 0.5),
            Vec3::new(-2.0, 7.0, -4.0),
            Vec3::new(0.001, 0.002, -0.003),
            Vec3::new(11.0, 11.0, 11.0),
        ];
        let mut corr = vec![Vec3::ZERO; pos.len()];
        ftl_pass(&mut pos, &mut corr, l0);
        for i in 0..pos.len() - 1 {
            let len = (pos[i + 1] - pos[i]).length();
            assert!((len - l0).abs() < 1.0e-5, "segment {i} is {len}, want {l0}");
        }
    }

    #[test]
    fn ftl_pass_never_moves_the_root() {
        let mut pos = vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(9.0, 9.0, 9.0)];
        let mut corr = vec![Vec3::ZERO; 2];
        ftl_pass(&mut pos, &mut corr, 0.05);
        assert_eq!(pos[0], Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(corr[0], Vec3::ZERO);
    }

    /// Eq. 9 must be inert when FTL corrected nothing — otherwise it is not a correction, it is a bias.
    #[test]
    fn the_velocity_correction_is_a_no_op_when_nothing_was_corrected() {
        let pos = vec![Vec3::ZERO, Vec3::new(0.0, -0.05, 0.0), Vec3::new(0.0, -0.10, 0.0)];
        let old = vec![Vec3::ZERO, Vec3::new(0.0, -0.04, 0.0), Vec3::new(0.0, -0.09, 0.0)];
        let corr = vec![Vec3::ZERO; 3];
        let mut vel = vec![Vec3::ZERO; 3];
        let dt = 1.0 / 240.0;
        dftl_velocities(&pos, &old, &corr, &mut vel, dt, 0.9);
        for i in 1..3 {
            let want = (pos[i] - old[i]) / dt;
            assert!((vel[i] - want).length() < 1.0e-6, "particle {i}: {:?} != {want:?}", vel[i]);
        }
    }

    /// The reason Eq. 9 exists, measured rather than asserted: FTL alone injects momentum into the
    /// leader, and the correction term is what takes it back out.
    #[test]
    fn the_dftl_velocity_correction_removes_the_ftl_momentum() {
        let pos = vec![Vec3::ZERO, Vec3::new(0.0, -0.05, 0.0), Vec3::new(0.0, -0.10, 0.0)];
        let old = vec![Vec3::ZERO, Vec3::new(0.0, -0.05, 0.0), Vec3::new(0.0, -0.10, 0.0)];
        // The tip was hauled back 2 cm by its own FTL projection this substep.
        let corr = vec![Vec3::ZERO, Vec3::ZERO, Vec3::new(0.0, 0.02, 0.0)];
        let dt = 1.0 / 240.0;

        let mut raw = vec![Vec3::ZERO; 3];
        dftl_velocities(&pos, &old, &corr, &mut raw, dt, 0.0);
        let mut damped = vec![Vec3::ZERO; 3];
        dftl_velocities(&pos, &old, &corr, &mut damped, dt, 0.9);

        // Particle 1 is the leader of the corrected tip, so it is the one Eq. 9 acts on.
        assert!(raw[1].length() < 1.0e-6, "with s=0 the leader keeps the momentum FTL invented");
        assert!(
            damped[1].length() > 1.0,
            "with s=0.9 the leader must absorb the successor's correction, got {:?}",
            damped[1]
        );
    }

    /// Ported from the superseded solver, tolerance tightened from 15% to 1e-5 — which is the whole
    /// point of moving to FTL.
    #[test]
    fn a_pinned_guide_settles_without_stretching_past_rest_length() {
        let cfg = settings();
        let g = settled(&cfg, 300, Vec3::ZERO);
        assert_eq!(g.pos[0], Vec3::ZERO, "root must stay pinned exactly at the host");
        for i in 0..g.pos.len() - 1 {
            let len = (g.pos[i + 1] - g.pos[i]).length();
            assert!(
                (len - cfg.rest_length).abs() < 1.0e-5,
                "segment {i} length {len} strayed from rest {}",
                cfg.rest_length
            );
        }
        for p in &g.pos {
            assert!(p.is_finite(), "a settled guide must never produce NaN/Inf: {p:?}");
        }
    }

    /// A settled guide whose root then TELEPORTS must re-settle, not fly apart.
    ///
    /// This is the case the player hit: a squad unit is respawned by the `RunState::Idle → Active`
    /// world rebuild, so its head bone jumps tens of metres in one frame, and the chain explodes into
    /// cards stretched across the level. The name is kept from the superseded solver because
    /// `src/rig_watch.rs`'s module doc cites it as the disproven hypothesis for the stretched-spike
    /// artifact; renaming it would break that archaeology.
    ///
    /// The assertion is *tightened*: DFTL plus [`is_teleport`] recovers on the FIRST frame after the
    /// jump, not the 120th.
    #[test]
    fn a_teleported_root_re_settles_instead_of_exploding() {
        let cfg = settings();
        let mut g = settled(&cfg, 120, Vec3::ZERO);
        let mut scratch = Vec::new();

        // The rebuild: the head bone lands 80 units away in a single frame.
        let far = Vec3::new(80.0, 0.0, 45.0);
        let reseeded = step_guide(&mut g, far, Vec3::NEG_Y, 1.0 / 60.0, 0.0, &cfg, &mut scratch);
        assert!(reseeded, "an 80 m root jump must be recognised as a teleport, not integrated");

        for (i, p) in g.pos.iter().enumerate() {
            assert!(p.is_finite(), "particle {i} is non-finite after a teleport: {p:?}");
        }
        for i in 0..g.pos.len() - 1 {
            let len = (g.pos[i + 1] - g.pos[i]).length();
            assert!(
                (len - cfg.rest_length).abs() < 1.0e-5,
                "segment {i} is {len} long, {:.0}x its rest length {} — the chain exploded",
                len / cfg.rest_length,
                cfg.rest_length
            );
        }
        // …and it must have actually followed the root, not merely stayed short somewhere else.
        let tip_to_root = (g.pos[g.pos.len() - 1] - far).length();
        assert!(
            tip_to_root <= g.span(cfg.rest_length) + 1.0e-5,
            "the chain did not follow its root: tip is {tip_to_root} from the host"
        );
    }

    /// The gap DFTL alone leaves: a reseed that kept its velocity would whip on the very next frame.
    #[test]
    fn the_teleport_reseed_zeroes_velocity_so_the_next_frame_does_not_whip() {
        let cfg = settings();
        let mut g = settled(&cfg, 120, Vec3::ZERO);
        let mut scratch = Vec::new();
        let far = Vec3::new(80.0, 0.0, 45.0);

        step_guide(&mut g, far, Vec3::NEG_Y, 1.0 / 60.0, 0.0, &cfg, &mut scratch);
        assert!(g.vel.iter().all(|v| *v == Vec3::ZERO), "a reseed must leave the guide at rest");

        step_guide(&mut g, far, Vec3::NEG_Y, 1.0 / 60.0, 0.0, &cfg, &mut scratch);
        for (i, v) in g.vel.iter().enumerate() {
            assert!(v.length() < cfg.max_speed, "particle {i} whipped to {} m/s", v.length());
        }
    }

    /// A root moving fast but *plausibly* must be followed, not mistaken for a teleport — otherwise a
    /// sprinting unit's hair resets every frame and never simulates at all.
    #[test]
    fn a_fast_but_followable_root_is_not_treated_as_a_teleport() {
        let cfg = settings();
        let span = cfg.rest_length * cfg.segments_per_strand as f32;
        // 3 m/s at 60 fps is 5 cm per frame; the strand is 25 cm long.
        assert!(!is_teleport(Vec3::ZERO, Vec3::X * 0.05, span));
        assert!(is_teleport(Vec3::ZERO, Vec3::X * 80.0, span));
        assert!(is_teleport(Vec3::ZERO, Vec3::new(f32::NAN, 0.0, 0.0), span));
    }

    /// Damping is expressed per SECOND, so the settled pose must not depend on the tick rate. The
    /// superseded solver damped per substep and silently changed look whenever `substeps` moved.
    #[test]
    fn damping_is_frame_rate_independent() {
        let cfg = settings();
        let mut scratch = Vec::new();

        let mut slow = Guide::new(cfg.segments_per_strand, test_bind(), 0.0);
        reseed(&mut slow, Vec3::ZERO, Vec3::X, cfg.rest_length);
        for _ in 0..(30 * 2) {
            step_guide(&mut slow, Vec3::ZERO, Vec3::X, 1.0 / 30.0, 0.0, &cfg, &mut scratch);
        }

        let mut fast = Guide::new(cfg.segments_per_strand, test_bind(), 0.0);
        reseed(&mut fast, Vec3::ZERO, Vec3::X, cfg.rest_length);
        for _ in 0..(240 * 2) {
            step_guide(&mut fast, Vec3::ZERO, Vec3::X, 1.0 / 240.0, 0.0, &cfg, &mut scratch);
        }

        for i in 0..slow.pos.len() {
            let d = (slow.pos[i] - fast.pos[i]).length();
            assert!(d < 1.0e-2, "particle {i} settled {d} apart between 30 Hz and 240 Hz");
        }
    }

    /// The solver must never emit NaN, whatever it is handed. This is the `rig_watch::joint_fault`
    /// discipline applied to the solver that `rig_watch` exists because of.
    #[test]
    fn a_guide_never_goes_non_finite_under_absurd_input() {
        let mut cfg = settings();
        let mut scratch = Vec::new();
        for (dt, gravity, dir) in [
            (1.0 / 60.0, 1.0e9_f32, Vec3::NEG_Y),
            (1.0e-6, 9.8, Vec3::ZERO),
            (0.5, 9.8, Vec3::NEG_Y),
            (1.0 / 60.0, 0.0, Vec3::ZERO),
        ] {
            cfg.gravity = gravity;
            let mut g = Guide::new(cfg.segments_per_strand, test_bind(), 0.0);
            reseed(&mut g, Vec3::ZERO, dir, cfg.rest_length);
            for _ in 0..120 {
                step_guide(&mut g, Vec3::ZERO, dir, dt, 0.0, &cfg, &mut scratch);
            }
            for (i, p) in g.pos.iter().enumerate() {
                assert!(p.is_finite(), "particle {i} non-finite at dt={dt} gravity={gravity}: {p:?}");
            }
            for (i, v) in g.vel.iter().enumerate() {
                assert!(v.is_finite(), "velocity {i} non-finite at dt={dt} gravity={gravity}: {v:?}");
            }
        }
    }

    /// A zero growth direction would collapse every particle onto the root, which FTL cannot recover
    /// from — it would have no direction to project along.
    #[test]
    fn reseed_with_a_degenerate_direction_still_produces_a_straight_strand() {
        let cfg = settings();
        let mut g = Guide::new(cfg.segments_per_strand, test_bind(), 0.0);
        reseed(&mut g, Vec3::ZERO, Vec3::ZERO, cfg.rest_length);
        for i in 0..g.pos.len() - 1 {
            let len = (g.pos[i + 1] - g.pos[i]).length();
            assert!((len - cfg.rest_length).abs() < 1.0e-6, "segment {i} collapsed to {len}");
        }
    }

    #[test]
    fn wind_is_stronger_at_the_tip_than_the_root() {
        let mut cfg = settings();
        cfg.wind_strength = 1.0;
        let root = wind_accel(0.3, 0, 1.0, &cfg).length();
        let tip = wind_accel(0.3, 5, 1.0, &cfg).length();
        assert!(tip >= root, "tip (free end) should sway at least as much as the root, got root={root} tip={tip}");
    }

    /// Bend must resist curl, and must never cost inextensibility — that is why it runs before FTL.
    ///
    /// Isolated deliberately: no gravity, no wind, started from a tight arc. An earlier version of
    /// this test ran under gravity and strong wind and measured tip-to-root distance, which is not a
    /// curl measure at all under those forces — a FLOPPY chain hangs straight down and scores as
    /// "straight", so the test asserted the opposite of the truth and failed against a correct solver.
    #[test]
    fn a_stiffer_bend_straightens_a_curled_guide_without_stretching_it() {
        let mut cfg = settings();
        cfg.gravity = 0.0;
        cfg.wind_strength = 0.0;

        let extension = |bend: f32| {
            let mut c = cfg.clone();
            c.bend_compliance = bend;
            let mut g = Guide::new(8, test_bind(), 0.0);
            // A tight arc: each segment turns 0.7 rad from the last, coiling the strand up.
            let mut dir = Vec3::NEG_Y;
            for i in 1..g.pos.len() {
                g.pos[i] = g.pos[i - 1] + dir * c.rest_length;
                dir = Quat::from_rotation_z(0.7) * dir;
            }
            let mut scratch = Vec::new();
            for _ in 0..600 {
                step_guide(&mut g, Vec3::ZERO, Vec3::NEG_Y, 1.0 / 60.0, 0.0, &c, &mut scratch);
            }
            // FTL owns length regardless of how the bend constraint is tuned. Assert that here rather
            // than in a separate test, because it is precisely the ordering claim being made.
            for i in 0..g.pos.len() - 1 {
                let len = (g.pos[i + 1] - g.pos[i]).length();
                assert!((len - c.rest_length).abs() < 1.0e-5, "bend={bend} stretched segment {i} to {len}");
            }
            (g.pos[g.pos.len() - 1] - g.pos[0]).length()
        };

        let stiff = extension(1.0e-6);
        let floppy = extension(100.0);
        assert!(
            stiff > floppy * 2.0,
            "a stiffer bend must uncoil the strand: stiff={stiff} floppy={floppy}"
        );
    }
}
