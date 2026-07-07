//! Perceptual image comparison for the FX/render layer (feature `test-harness`).
//!
//! The gore / juice / VHS / blood-lens layer is driven by physics floats that are *not* bit-reproducible
//! (see the testing strategy), so its screenshots must be compared with a **perceptual tolerance**, never
//! exact bytes (RESP 2026; Wang et al. 2004, "Image Quality Assessment: From Error Visibility to
//! Structural Similarity" — the SSIM paper). This module is the reusable heart of that check: a
//! hand-rolled **SSIM** over grayscale buffers (no image-crate dependency in the core), returning a score
//! in `[-1, 1]` where `1.0` is identical. A regression asserts `ssim(golden, shot) >= threshold`.
//!
//! ## Golden-screenshot flow (Stage 5)
//! The capture half needs the *windowed* game + `devshot` (the headless sim harness has no window, so it
//! can't screenshot): run a fixed seed + scripted sequence, `touch screenshot.request` at set ticks,
//! then decode `screenshot.png` to grayscale and call [`ssim`] against a committed golden PNG. PNG decode
//! is the one piece that needs a dev-dependency (`image`); the SSIM math here is dependency-free and
//! fully unit-tested, so the perceptual oracle itself is verified even without the capture rig.

/// Mean SSIM between two equal-sized grayscale images (row-major, values in `[0, 1]`), computed over
/// non-overlapping 8×8 windows and averaged (Wang et al. 2004). Windows partially off the right/bottom
/// edge are skipped. Returns `1.0` for identical inputs; lower as structure/luminance/contrast diverge.
/// A tolerance threshold (e.g. `>= 0.98`) turns this into a regression gate that ignores the sub-pixel
/// jitter of non-reproducible FX while still catching real visual breakage.
pub fn ssim(a: &[f32], b: &[f32], width: usize, height: usize) -> f32 {
    assert_eq!(a.len(), width * height, "buffer a size mismatch");
    assert_eq!(b.len(), width * height, "buffer b size mismatch");
    // Stabilising constants for dynamic range L = 1.0 (the paper's defaults k1=0.01, k2=0.03).
    const C1: f32 = 0.01 * 0.01;
    const C2: f32 = 0.03 * 0.03;
    const WIN: usize = 8;

    let mut sum = 0.0f64;
    let mut windows = 0u32;
    let mut wy = 0;
    while wy + WIN <= height {
        let mut wx = 0;
        while wx + WIN <= width {
            let (mut ma, mut mb) = (0.0f32, 0.0f32);
            for j in 0..WIN {
                for i in 0..WIN {
                    let idx = (wy + j) * width + (wx + i);
                    ma += a[idx];
                    mb += b[idx];
                }
            }
            let n = (WIN * WIN) as f32;
            ma /= n;
            mb /= n;

            let (mut va, mut vb, mut cov) = (0.0f32, 0.0f32, 0.0f32);
            for j in 0..WIN {
                for i in 0..WIN {
                    let idx = (wy + j) * width + (wx + i);
                    let da = a[idx] - ma;
                    let db = b[idx] - mb;
                    va += da * da;
                    vb += db * db;
                    cov += da * db;
                }
            }
            // Sample variance/covariance (÷ n-1), matching the SSIM reference.
            let denom = n - 1.0;
            va /= denom;
            vb /= denom;
            cov /= denom;

            let s = ((2.0 * ma * mb + C1) * (2.0 * cov + C2))
                / ((ma * ma + mb * mb + C1) * (va + vb + C2));
            sum += s as f64;
            windows += 1;
            wx += WIN;
        }
        wy += WIN;
    }

    if windows == 0 {
        // Image smaller than one window — fall back to a single global comparison.
        return global_ssim(a, b, C1, C2);
    }
    (sum / windows as f64) as f32
}

/// Single-window SSIM over the whole buffer (fallback for images smaller than 8×8).
fn global_ssim(a: &[f32], b: &[f32], c1: f32, c2: f32) -> f32 {
    let n = a.len() as f32;
    let ma = a.iter().sum::<f32>() / n;
    let mb = b.iter().sum::<f32>() / n;
    let (mut va, mut vb, mut cov) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        let da = x - ma;
        let db = y - mb;
        va += da * da;
        vb += db * db;
        cov += da * db;
    }
    let denom = (n - 1.0).max(1.0);
    va /= denom;
    vb /= denom;
    cov /= denom;
    ((2.0 * ma * mb + c1) * (2.0 * cov + c2)) / ((ma * ma + mb * mb + c1) * (va + vb + c2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic checkerboard-ish gradient, no RNG crate.
    fn pattern(width: usize, height: usize, phase: f32) -> Vec<f32> {
        (0..width * height)
            .map(|k| {
                let x = (k % width) as f32;
                let y = (k / width) as f32;
                0.5 + 0.5 * ((x * 0.3 + y * 0.2 + phase).sin())
            })
            .collect()
    }

    #[test]
    fn identical_images_score_one() {
        let img = pattern(64, 48, 0.0);
        let s = ssim(&img, &img, 64, 48);
        assert!((s - 1.0).abs() < 1.0e-5, "identical images must score ~1.0, got {s}");
    }

    #[test]
    fn tiny_perturbation_stays_above_threshold() {
        // A small brightness jitter (the kind non-reproducible FX produce) keeps SSIM near 1 — a 0.98
        // tolerance would pass.
        let a = pattern(64, 48, 0.0);
        let b: Vec<f32> = a.iter().map(|v| (v + 0.01).clamp(0.0, 1.0)).collect();
        let s = ssim(&a, &b, 64, 48);
        assert!(s > 0.98, "a tiny perturbation should stay above a 0.98 gate, got {s}");
    }

    #[test]
    fn structural_change_scores_low() {
        // A phase-shifted pattern is structurally different → SSIM well below the gate.
        let a = pattern(64, 48, 0.0);
        let b = pattern(64, 48, 2.0);
        let s = ssim(&a, &b, 64, 48);
        assert!(s < 0.9, "a real structural change must score low, got {s}");
    }

    #[test]
    fn symmetric_and_deterministic() {
        let a = pattern(32, 32, 0.0);
        let b = pattern(32, 32, 0.7);
        assert_eq!(ssim(&a, &b, 32, 32), ssim(&a, &b, 32, 32));
        assert!((ssim(&a, &b, 32, 32) - ssim(&b, &a, 32, 32)).abs() < 1.0e-6, "SSIM is symmetric");
    }
}
