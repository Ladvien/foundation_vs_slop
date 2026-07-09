//! Dev instrument: **measure** what the mold's growth actually looks like, in the units the eye uses.
//!
//! `sim_hz` cannot be derived from first principles. An agent steps `step_size × sim_hz` = 1.125 world
//! units/s, some 340× over budget — but agents are *not rendered*. What the eye sees is the `display`
//! texture: a trail field so heavily smoothed by `diffuse_weight` that no individual agent is resolvable,
//! and a Gray-Scott biomass margin. The velocity of the visible feature is an emergent property of the
//! whole chain. So we measure it.
//!
//! # What it reports
//!
//! **Front speed** — how fast the visible biomass margin sweeps across the floor, in world units per
//! second, judged against `v_max` at the tightest zoom the player can reach (see [`super::perceptual`]).
//! This is the number that sets `sim_hz`. It is a purely geometric quantity: it does not depend on how the
//! margin is shaded, only on where it is.
//!
//! **Field churn** — how fast the raw trail and biomass fields swing. Reported as a *diagnostic only*.
//! It is tempting to compare it against `1 / MIN_APPEARANCE_RAMP_SECS` and call the mold 40× too flickery,
//! and an earlier version of this module did. That is wrong: the material's `smoothstep` ramps discard most
//! of the swing, the coating is translucent, the fbm fibre layer modulates it, and LDR tonemapping
//! compresses the rest. Diffing two *rendered* frames a second apart puts the mold's on-screen change at
//! the renderer's own frame-to-frame noise floor. Use churn to compare tunings; use the rendered frame to
//! decide whether something is visible.
//!
//! # What it found
//!
//! Measured at matched colony maturity (~66,000 texels covered, i.e. 450 sim ticks in):
//!
//! | `sim_hz` | front speed | vs. budget (3.33 mm/s @ `MIN_ZOOM`) |
//! |---|---|---|
//! | 6.0 | 23.1 mm/s | 6.9× over |
//! | 3.0 | 11.2 mm/s | 3.4× over |
//! | 1.5 | 2.92 mm/s | **0.88× — just under** |
//!
//! So the shipped mold was *seven times too fast to be invisible*, not too slow. The clock came down to
//! 1.5 Hz. Notably `decay` and `deposit_amount` were left alone: the binding constraint turned out to be
//! the Gray-Scott biomass margin, not the Physarum trail churn, and the trail settled along with the clock.
//!
//! # Running it
//!
//! ```sh
//! MYCELIA_MEASURE=1 cargo run     # logs a measurement every second
//! ```
//!
//! Two cautions when reading the log. **Compare at equal tick counts, not equal wall-clock** — a young
//! colony's front is much faster than a mature one's, so a slower clock measured at the same wall-clock
//! second is being judged during its expansion phase. And the one-second sampling window **aliases against
//! the tick rate**: at 1.5 Hz alternate windows contain one or two ticks, which is why the reported speed
//! alternates between ~1.8 and ~4.2 mm/s. Average over many samples.
//!
//! Off by default and gated on the environment variable, because it attaches a
//! [`bevy::render::gpu_readback::Readback`] to the full `field_size²` RGBA16F display texture — 8 MB a
//! frame at 1024². That is far too expensive to ship, and exactly cheap enough to calibrate with.

use std::env;

use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::time::Real;

use super::perceptual::{v_max, MIN_APPEARANCE_RAMP_SECS};
use super::{MoldImages, MyceliaConfig, WORLD_EXTENT};

/// Environment variable that arms the instrument.
const ENV_FLAG: &str = "MYCELIA_MEASURE";

/// Seconds between the two frames a measurement compares. One second is long enough that a subthreshold
/// front moves a measurable number of texels, and short enough that the front does not curve appreciably.
const CAPTURE_INTERVAL: f32 = 1.0;

/// Biomass `V` at which the mold first becomes visible. Matches the lower edge of `smoothstep(0.10, 0.35,
/// f.g)` in `mycelia_floor.wgsl` / `mycelia_wall.wgsl` — this is the isolevel the player's eye tracks, so
/// it is the contour whose speed matters, not some arbitrary threshold.
const COVER_ISO: f32 = 0.10;

/// Span of biomass over which the coating goes from invisible to fully opaque, from the same `smoothstep`.
/// Used to express a change in `V` as a change in apparent contrast.
const BIO_CONTRAST_SPAN: f32 = 0.25;

/// How close to [`COVER_ISO`] a texel must sit to count as "on the contour" for the level-set estimate.
/// Wide enough to catch the contour wherever it lies between texel centres, narrow enough that the sample
/// is genuinely local to it.
const CONTOUR_BAND: f32 = 0.02;

/// Minimum `|∇V|` (per world unit) for a contour sample to be usable. The normal speed is `|∂V/∂t| / |∇V|`,
/// so a near-flat field would divide a small numerator by a smaller denominator and manufacture a large
/// speed out of nothing. At the shipped `(feed, kill)` the margin's gradient is orders of magnitude above
/// this, so the cut discards noise rather than signal.
const MIN_GRADIENT: f32 = 0.20;

/// One decoded frame of the display texture: the two channels that carry visible signal.
struct Frame {
    /// Seconds since startup, on the real clock the mold runs on.
    t: f32,
    /// `R` — raw trail intensity, `0..trail_max`.
    trail: Vec<f32>,
    /// `G` — Gray-Scott biomass `V`, `0..1`.
    v: Vec<f32>,
}

#[derive(Resource, Default)]
struct Measure {
    /// Set by the timer; the observer decodes only on frames where this is true, so the (expensive) decode
    /// happens once per interval rather than once per rendered frame.
    want: bool,
    previous: Option<Frame>,
}

pub(super) fn build(app: &mut App) {
    if env::var(ENV_FLAG).is_err() {
        return;
    }
    warn!(
        "mycelia: {ENV_FLAG} is set — attaching a full-resolution display readback. This is a calibration \
         tool and costs several MB per frame; do not ship with it enabled."
    );
    app.init_resource::<Measure>()
        .add_systems(Startup, arm.after(super::setup_mycelia))
        .add_systems(Update, tick);
}

fn arm(mut commands: Commands, images: Res<MoldImages>) {
    commands
        .spawn((Name::new("mycelia_measure_readback"), Readback::texture(images.display.clone())))
        .observe(capture);
}

/// Ask for a decode once per [`CAPTURE_INTERVAL`].
fn tick(time: Res<Time<Real>>, mut measure: ResMut<Measure>, mut next: Local<f32>) {
    let now = time.elapsed_secs();
    if now >= *next {
        *next = now + CAPTURE_INTERVAL;
        measure.want = true;
    }
}

/// Decode a display frame and, once we hold two, report the two numbers that govern the whole design.
fn capture(
    trigger: On<ReadbackComplete>,
    cfg: Res<MyceliaConfig>,
    time: Res<Time<Real>>,
    mut measure: ResMut<Measure>,
) -> Result<(), BevyError> {
    if !measure.want {
        return Ok(());
    }
    measure.want = false;

    let size = cfg.field_size as usize;
    let texels = size * size;
    let bytes = &trigger.event().data;
    // Rgba16Float: 4 channels × 2 bytes. At 1024 wide the row is 8192 B, already a multiple of wgpu's
    // 256-byte copy alignment, so the readback carries no row padding to strip. Assert rather than assume.
    let expected = texels * 4 * 2;
    if bytes.len() != expected {
        return Err(format!(
            "mycelia measure: display readback is {} bytes, expected {expected} ({size}² × RGBA16F). If \
             field_size stopped being a multiple of 128 the rows are now padded and this decode is wrong.",
            bytes.len()
        )
        .into());
    }

    let mut trail = Vec::with_capacity(texels);
    let mut v = Vec::with_capacity(texels);
    for i in 0..texels {
        let o = i * 8;
        trail.push(f16_to_f32(u16::from_le_bytes([bytes[o], bytes[o + 1]])));
        v.push(f16_to_f32(u16::from_le_bytes([bytes[o + 2], bytes[o + 3]])));
    }
    let current = Frame { t: time.elapsed_secs(), trail, v };

    if let Some(previous) = measure.previous.take() {
        report(&cfg, &previous, &current, size);
    }
    measure.previous = Some(current);
    Ok(())
}

fn report(cfg: &MyceliaConfig, previous: &Frame, current: &Frame, size: usize) {
    let dt = current.t - previous.t;
    if dt <= 0.0 {
        return;
    }
    let texel_world = WORLD_EXTENT.x / size as f32;

    // ── Front speed, by the level-set formula ─────────────────────────────────────────────────────────
    //
    // The visible margin is the contour `V = COVER_ISO`. For a level set of `φ = V − ISO`, the contour's
    // speed along its own normal is exactly `|∂V/∂t| / |∇V|`. This is the right estimator for three reasons:
    // it is a *speed* rather than a net displacement (a steady-state colony's advance and retreat cancel,
    // and a signed-area estimator would call a boiling margin placid); it has sub-texel resolution; and it
    // has no noise floor, whereas counting texels that flip a binary mask reports the isoline's numerical
    // dither as motion. An earlier mask-based estimator did exactly that and read ~2.4 mm/s even as `sim_hz`
    // went to zero.
    //
    // Sampled only near the contour and only where the gradient is steep enough to locate it — a flat field
    // has no contour to move, and dividing by a vanishing `|∇V|` would manufacture an infinite speed.
    // Gradient-weighted ratio of sums, NOT a median of per-texel ratios. Both estimate the same quantity,
    // but `mean(|∂V/∂t| / |∇V|)` is dominated by whichever samples happen to have the shallowest gradient —
    // it divides a small numerator by a smaller denominator and manufactures speed. `Σ|∂V/∂t| / Σ|∇V|`
    // weights each sample by how sharply it locates the contour, which is exactly the confidence it
    // deserves. (The median form read ~11 mm/s at both 6 Hz and 3 Hz — a number that could not possibly be
    // right, since every velocity in this system is proportional to the tick rate.)
    let mut sum_dvdt = 0.0f64;
    let mut sum_grad = 0.0f64;
    let mut samples = 0usize;
    for y in 1..size - 1 {
        for x in 1..size - 1 {
            let i = y * size + x;
            if (previous.v[i] - COVER_ISO).abs() > CONTOUR_BAND {
                continue;
            }
            // Central differences, in world units.
            let gx = (previous.v[i + 1] - previous.v[i - 1]) / (2.0 * texel_world);
            let gy = (previous.v[i + size] - previous.v[i - size]) / (2.0 * texel_world);
            let grad = (gx * gx + gy * gy).sqrt();
            if grad < MIN_GRADIENT {
                continue;
            }
            sum_dvdt += ((current.v[i] - previous.v[i]).abs() / dt) as f64;
            sum_grad += grad as f64;
            samples += 1;
        }
    }
    let front = if sum_grad > 0.0 { (sum_dvdt / sum_grad) as f32 } else { 0.0 };

    let area = (0..size * size).filter(|&i| current.v[i] > COVER_ISO).count();

    // ── Field-domain churn (a diagnostic, NOT a perceptual budget) ────────────────────────────────────
    //
    // How fast the raw fields swing, normalised by the range each maps onto visible contrast. This does
    // *not* convert into a perceptual number: the material's `smoothstep(vein_lo, vein_hi)` and
    // `smoothstep(0.10, 0.35)` ramps discard most of the swing, the coating is translucent, the fbm fibre
    // layer modulates it, and LDR tonemapping compresses what is left. Diffing two rendered frames a second
    // apart shows the mold's on-screen change sitting at the renderer's own frame-to-frame noise floor,
    // while these raw numbers read as tens of "contrast units" per second. Use them to compare tunings
    // against each other; use the rendered frame to decide whether something is visible.
    let mut peak_trail: f32 = 0.0;
    let mut peak_bio: f32 = 0.0;
    for i in 0..size * size {
        peak_trail = peak_trail.max((current.trail[i] - previous.trail[i]).abs() / cfg.vein_hi);
        peak_bio = peak_bio.max((current.v[i] - previous.v[i]).abs() / BIO_CONTRAST_SPAN);
    }
    let trail_rate = peak_trail / dt;
    let bio_rate = peak_bio / dt;

    // Judged at the tightest zoom the player can reach — the worst case, and the one the design is
    // anchored on. `sim_hz` is deliberately *not* zoom-adaptive (unlike a fruit body's growth clock),
    // because the reaction-diffusion regime should not depend on a UI input.
    let budget = v_max(cfg.motion_threshold_deg_per_s, cfg.screen_fov_deg_v, crate::camera::MIN_ZOOM);
    let verdict = if front <= budget { "under" } else { "OVER " };

    info!(
        "mycelia measure [dt={dt:.2}s  sim_hz={:.2}  decay={:.3}  deposit={:.3}]\n  \
         front speed  {:>8.4} mm/s   budget {:>6.4} mm/s @ MIN_ZOOM  [{}] ({:.2}x)\n  \
         field churn (diagnostic, not perceptual): trail {trail_rate:.3}/s  bio {bio_rate:.3}/s\n  \
         {samples} contour samples, {area} texels covered, appearance-ramp ceiling {:.4}/s",
        cfg.sim_hz,
        cfg.decay,
        cfg.deposit_amount,
        front * 1000.0,
        budget * 1000.0,
        verdict,
        front / budget,
        1.0 / MIN_APPEARANCE_RAMP_SECS,
    );
}

/// IEEE 754 binary16 → binary32. The display texture is `Rgba16Float` and Rust has no stable `f16`.
fn f16_to_f32(h: u16) -> f32 {
    let sign = (h >> 15) as u32;
    let exp = ((h >> 10) & 0x1f) as u32;
    let mant = (h & 0x3ff) as u32;

    // Subnormals (exp == 0, mant != 0) are exactly `mant × 2⁻²⁴`, which every f32 represents exactly. Doing
    // it in float arithmetic sidesteps the renormalisation bit-shuffle, where an off-by-one in the exponent
    // is easy to write and impossible to see.
    if exp == 0 {
        let magnitude = mant as f32 * 5.960_464_5e-8; // 2⁻²⁴
        return if sign == 1 { -magnitude } else { magnitude };
    }

    let bits = if exp == 31 {
        (sign << 31) | 0x7f80_0000 | (mant << 13) // ±inf / NaN
    } else {
        (sign << 31) | ((exp + 112) << 23) | (mant << 13)
    };
    f32::from_bits(bits)
}

#[cfg(test)]
mod tests {
    use super::f16_to_f32;

    /// Spot-check the half decoder against known bit patterns. A wrong decode would silently scale every
    /// measurement, and the whole point of this module is that its numbers can be trusted.
    #[test]
    fn half_precision_decodes_correctly() {
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert_eq!(f16_to_f32(0x8000), -0.0);
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xbc00), -1.0);
        assert_eq!(f16_to_f32(0x4000), 2.0);
        assert_eq!(f16_to_f32(0x3800), 0.5);
        assert_eq!(f16_to_f32(0x4c00), 16.0);
        assert_eq!(f16_to_f32(0x4e00), 24.0); // the shipped `trail_max`
        assert!((f16_to_f32(0x3555) - 0.333_251).abs() < 1e-5); // ~1/3
        assert!(f16_to_f32(0x7c00).is_infinite());
        assert!(f16_to_f32(0xfc00).is_infinite() && f16_to_f32(0xfc00) < 0.0);
        // Smallest positive subnormal: 2^-24.
        assert!((f16_to_f32(0x0001) - 5.960_464_5e-8).abs() < 1e-12);
        // Largest subnormal, just below the smallest normal 2^-14.
        assert!(f16_to_f32(0x03ff) < f16_to_f32(0x0400));
        assert!(f16_to_f32(0x03ff) > 0.0);
    }
}
