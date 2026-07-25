//! SCP-999 runtime soft-body — the "jelly physics" made of damped harmonic oscillators, one per gel morph
//! mode, struck by the blob's own acceleration + a tickle bounce + a gentle idle breath, and written onto
//! the glTF `MorphWeights`. This is **modal dynamics** (Pentland & Williams, "Good Vibrations: Modal
//! Dynamics for Graphics and Animation", SIGGRAPH 1989, DOI 10.1145/74334.74355): represent a deformable
//! body by a few vibration modes and integrate each as an independent spring, instead of a full FEM soft
//! body — two orders of magnitude cheaper and unconditionally stable at substepped `dt`. The reference
//! model + constants come from the asset hand-off (`assets/scp999/README.md` §5).
//!
//! Purely cosmetic: it writes only `MorphWeights` (never `(Transform, Health)`), runs on `PostUpdate`, and
//! is registered windowed-only (`Scp999VisualsPlugin`), so it can never touch `snapshot_hash`. It tolerates
//! `MorphWeights` being absent for the first frame or two while the glTF scene instantiates asynchronously
//! (the `get_mut` simply fails and we skip — same discipline as `mycelia::fruit::drive_morph_weights`).

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::Scp999Motion;
use crate::util::hash01_u32;

// Morph target order in scp-999.glb — verified against `mesh.extras.targetNames` at export (README §1).
const T_SQUASH: usize = 0;
const T_STRETCH: usize = 1;
const T_WOBBLE_X: usize = 2;
const T_WOBBLE_Y: usize = 3;
const T_PULSE: usize = 4;
const N_TARGETS: usize = 5;

/// Idle "breathing" swell of the pulse mode so a resting blob is never dead-still.
const IDLE_BREATH_HZ: f32 = 0.5;
const IDLE_BREATH_AMP: f32 = 0.45;
/// Continuous lateral "sway" on the two wobble modes — a slow *rolling* jiggle (two 90°-offset phases) so
/// the WHOLE body is always sloshing side-to-side like a bowl of jelly, not just breathing vertically.
const IDLE_SWAY_HZ: f32 = 0.6;
const IDLE_SWAY_AMP: f32 = 0.6;
/// Continuous vertical squash↔stretch "settle" — the blob sinks and spreads at the base, then gathers back
/// up, on repeat. This is the main slime read: a big, slow oozing of the whole body **especially around the
/// base** (the squash morph flattens + spreads the foot of the dome). Signed: negative → squash, positive → stretch.
const VERT_OSC_HZ: f32 = 0.42;
const VERT_OSC_AMP: f32 = 0.7;
/// Impulse kicked into the springs on the rising edge of a tickle — the giggle bounce.
const TICKLE_KICK: f32 = 2.2;
/// Clamp on the final blend weights. Deliberately well past 1.0 so the morphs can be pushed into big,
/// gooey, over-extended slime deformation (extrapolating the authored targets), not a stiff ±1 wobble.
const WEIGHT_MAX: f32 = 3.0;

/// One damped harmonic oscillator: `x'' + 2·ζ·ω·x' + ω²·x = 0`, struck by velocity impulses. Substepped
/// semi-implicit Euler — the `integrate_damped` model of `SCP_Characters/monsters/softbody.py`.
#[derive(Clone, Copy)]
struct Spring {
    x: f32,
    v: f32,
    omega: f32,
    zeta: f32,
}

impl Spring {
    fn new(hz: f32, zeta: f32) -> Self {
        Self { x: 0.0, v: 0.0, omega: TAU * hz, zeta }
    }
    fn kick(&mut self, impulse: f32) {
        self.v += impulse;
    }
    fn step(&mut self, dt: f32, substeps: u32) {
        step_damped(&mut self.x, &mut self.v, 0.0, self.omega, self.zeta, dt, substeps);
    }
}

/// One substepped semi-implicit-Euler advance of a damped harmonic oscillator relaxing toward `target`:
/// `x'' + 2·ζ·ω·(x' ) + ω²·(x − target) = 0`. THE integrator for this feature's springs — [`Spring::step`]
/// (relaxing to 0) and the eye-bounce springs in [`super::eyes`] (relaxing to the body's bounce, per axis)
/// are both this function, so a stability or damping fix lands in one place instead of two copies that
/// silently drift apart.
///
/// Semi-implicit Euler is only stable while `ω·h < 2`, so callers must hand it a CLAMPED `dt` (see
/// `drive_blob_jiggle`'s `step_dt`) — a long frame otherwise amplifies the oscillator instead of decaying
/// it. Substepping shrinks `h` for a given frame, buying headroom for the stiffer modes.
pub(super) fn step_damped(
    x: &mut f32,
    v: &mut f32,
    target: f32,
    omega: f32,
    zeta: f32,
    dt: f32,
    substeps: u32,
) {
    let h = dt / substeps as f32;
    for _ in 0..substeps {
        let a = -(omega * omega) * (*x - target) - 2.0 * zeta * omega * *v;
        *v += a * h;
        *x += *v * h;
    }
}

/// Per-blob soft-body state: one spring per authored morph mode, plus the motion-differencing state that
/// turns the blob's movement into spring impulses (so it wobbles reactively when it starts, stops, or is
/// jostled). `phase` decorrelates the idle breath between blobs; `tickle_prev` gives the tickle a rising
/// edge so a sustained contact bounces once, not every frame.
#[derive(Component)]
pub(crate) struct BlobJiggle {
    vertical: Spring, // squash (−) ↔ stretch (+)
    wobble_x: Spring,
    wobble_y: Spring, // Bevy is Y-up, so the ground plane is XZ; this is the lateral (Z) wobble
    pulse: Spring,
    prev_pos: Option<Vec3>,
    prev_vel: Vec3,
    accel_gain: f32,
    substeps: u32,
    phase: f32,
    tickle_prev: bool,
    /// The gross visible body deformation written last frame — `(lateral lean, vertical squash/stretch)`.
    /// Read by the eyes so they bounce *with* the body (see `eyes::update_scp999_eyes`).
    last_bounce: Vec2,
}

impl BlobJiggle {
    /// Construct with per-blob idle phase from the spawn seed. Frequencies from README §5; the damping is
    /// deliberately LOW (very underdamped) so a kick *rings* and sloshes for a good beat instead of snapping
    /// back — reads as loose, gooey slime, not stiff rubber.
    pub(crate) fn new(seed: u32) -> Self {
        Self {
            vertical: Spring::new(2.4, 0.09),
            wobble_x: Spring::new(1.7, 0.11),
            wobble_y: Spring::new(1.7, 0.11),
            pulse: Spring::new(3.1, 0.14),
            prev_pos: None,
            prev_vel: Vec3::ZERO,
            // Per-SECOND gain: `drive_blob_jiggle` multiplies it by the frame's timestep, so the authored
            // 0.14-per-frame feel is preserved by rescaling to the 60 Hz reference it was tuned at
            // (0.14 × 60 = 8.4) — same look at 60 Hz, now identical at any other refresh rate.
            accel_gain: 8.4,
            substeps: 8,
            phase: hash01_u32(seed) * TAU,
            tickle_prev: false,
            last_bounce: Vec2::ZERO,
        }
    }

    /// The blob's current gross deformation — `(lateral lean, vertical squash/stretch)`. The eyes track a
    /// scaled version of this with detuned per-eye springs, so each eye bounces along with the jiggle.
    pub(crate) fn bounce(&self) -> Vec2 {
        self.last_bounce
    }
}

/// Excite each spring from the blob's acceleration + the tickle bounce + an idle breath, integrate, and
/// write the result onto the gel's `MorphWeights`. Ordered on `PostUpdate` (after gameplay set this tick's
/// Transform). Writes weights **absolutely** (there is no baked clip to layer on in this build — see the
/// README for the alternative baked-`settle` path).
pub(crate) fn drive_blob_jiggle(
    time: Res<Time>,
    mut blobs: Query<(Entity, &GlobalTransform, &Scp999Motion, &mut BlobJiggle)>,
    children: Query<&Children>,
    mut weights: Query<&mut MorphWeights>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    // Spring-integration window, clamped like every other per-frame integrator here (`scp999::movement`,
    // `crab`, `enemy`). `Spring::step` is semi-implicit Euler, which is only stable while `omega·h < 2`:
    // the stiffest mode (pulse, omega = 2π·3.1 ≈ 19.5 rad/s) at 8 substeps needs a frame under ~0.8 s, so
    // one hitch (shader compile, window drag, a debugger pause) would AMPLIFY the springs instead of
    // decaying them and slam the morph weights to their ±WEIGHT_MAX clamp — the blob visibly detonating.
    // NOTE the raw `dt` is deliberately kept for the velocity/acceleration differencing below: dividing a
    // hitch's large displacement by the *clamped* dt would manufacture a huge phantom acceleration, which
    // is the very impulse this clamp exists to prevent.
    let step_dt = dt.min(super::MAX_FRAME_DT);
    let elapsed = time.elapsed_secs();
    for (root, gxf, motion, mut j) in &mut blobs {
        // Difference the world position twice → acceleration; strike the springs with it so the blob
        // compresses on a stop/landing and stretches when it surges (README §5).
        let pos = gxf.translation();
        let vel = match j.prev_pos {
            Some(p) => (pos - p) / dt,
            None => Vec3::ZERO,
        };
        let accel = (vel - j.prev_vel) / dt;
        j.prev_pos = Some(pos);
        j.prev_vel = vel;

        // Acceleration kicks are scaled by the timestep: `kick` adds to VELOCITY, so a continuous
        // acceleration must contribute `a·g·dt` per frame to inject the same impulse per second at any
        // frame rate. Unscaled, the per-frame kick was `a·g` while `a` is itself a `Δv/dt` difference, so
        // the energy handed to the springs scaled with refresh rate — the same shove wobbled the blob
        // visibly harder on a 144 Hz monitor than on a 60 Hz one. (The discrete TICKLE_KICK below is a
        // true one-shot impulse on a rising edge, so it is correctly NOT dt-scaled.)
        let g = j.accel_gain * step_dt;
        j.vertical.kick(-accel.y * g); // (mostly 0 on the ground plane; kept for landings/knocks)
        j.wobble_x.kick(accel.x * g);
        j.wobble_y.kick(accel.z * g);
        j.pulse.kick(accel.length() * g * 0.5);

        // Tickle bounce on the rising edge: a delighted giggle-quiver when it first makes contact.
        if motion.tickling && !j.tickle_prev {
            j.vertical.kick(TICKLE_KICK);
            j.pulse.kick(TICKLE_KICK * 0.8);
        }
        j.tickle_prev = motion.tickling;

        let substeps = j.substeps;
        j.vertical.step(step_dt, substeps);
        j.wobble_x.step(step_dt, substeps);
        j.wobble_y.step(step_dt, substeps);
        j.pulse.step(step_dt, substeps);

        // Continuous idle deformation, layered on top of the reactive springs — this is what makes it read
        // as loose slime rather than a stiff body. Uses wall-clock `elapsed` (cosmetic, no determinism tie).
        //   breath: a slow swell of the pulse mode.
        //   sway:   a rolling side-to-side slosh of the whole body (two 90°-offset lateral phases).
        //   vosc:   a big vertical squash↔stretch "settle" — the body sinks + spreads at the base, then
        //           gathers, on repeat. The main slime read, and the base-deformation the design asks for.
        let breath = IDLE_BREATH_AMP * (0.5 - 0.5 * (elapsed * IDLE_BREATH_HZ * TAU + j.phase).cos());
        let sway_t = elapsed * IDLE_SWAY_HZ * TAU + j.phase;
        let sway_x = IDLE_SWAY_AMP * sway_t.sin();
        let sway_y = IDLE_SWAY_AMP * (sway_t + std::f32::consts::FRAC_PI_2).sin();
        let vosc = VERT_OSC_AMP * (elapsed * VERT_OSC_HZ * TAU + j.phase * 1.7).sin();

        // Combine reactive springs + continuous idle into the final per-mode signals.
        let vert = j.vertical.x + vosc; // signed: <0 squash (base spreads), >0 stretch (rises)
        let wob_x = j.wobble_x.x + sway_x;
        let wob_y = j.wobble_y.x + sway_y;
        let pulse = j.pulse.x.max(0.0) + breath;
        // Record the gross deformation so the eyes can bounce along with it (lateral lean, vertical).
        j.last_bounce = Vec2::new(wob_x, vert);

        // Find the descendant carrying MorphWeights (the glTF gel node); absent for a frame or two after
        // spawn while the scene instantiates — skip until it appears.
        for descendant in children.iter_descendants(root) {
            let Ok(mut mw) = weights.get_mut(descendant) else {
                continue;
            };
            let w = mw.weights_mut();
            if w.len() < N_TARGETS {
                continue;
            }
            w[T_SQUASH] = (-vert).max(0.0).min(WEIGHT_MAX);
            w[T_STRETCH] = vert.max(0.0).min(WEIGHT_MAX);
            w[T_WOBBLE_X] = wob_x.clamp(-WEIGHT_MAX, WEIGHT_MAX);
            w[T_WOBBLE_Y] = wob_y.clamp(-WEIGHT_MAX, WEIGHT_MAX);
            w[T_PULSE] = pulse.min(WEIGHT_MAX);
            break; // exactly one node carries MorphWeights; the primitives reference it
        }
    }
}
