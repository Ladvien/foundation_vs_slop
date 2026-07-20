//! Windowed SSIM visual-regression capture — the automated capture rig for the `visual_regression::ssim`
//! oracle (2026-07-19 review Finding F; the one gap the review flagged in the test pyramid). It launches
//! the real windowed game, drives a `devshot` screenshot via the `screenshot.request` sentinel, decodes
//! the PNG, and compares it (SSIM) against a committed golden of the title screen.
//!
//! `#[ignore]` because it needs a **real window/display** — the headless harness renders nothing, and CI
//! without a GPU/display cannot run it. Run it on a display-equipped box:
//!   `cargo test --features test-harness --test visual_capture -- --ignored`
//!
//! SSIM = Wang, Bovik, Sheikh & Simoncelli, "Image Quality Assessment: From Error Visibility to
//! Structural Similarity", IEEE TIP 13(4):600–612, 2004 (the basis of `visual_regression::ssim`). The
//! record-then-replay-and-compare shape is the automated-visual-testing pattern surveyed in Politowski,
//! Petrillo & Guéhéneuc, "A Survey of Video Game Testing", arXiv:2103.06431.
//!
//! **Regenerating the golden** (after an intentional title-screen art change): capture a clean title
//! frame (`touch screenshot.request`; see `CLAUDE.md` → "Taking screenshots"), then downscale it to
//! `W`×`H` grayscale and overwrite `tests/golden/title_screen.png` (a ~60 KB image, not the ~8 MB native
//! frame). The capture is resized to the same `W`×`H` here, so the golden is resolution-independent — it
//! matches regardless of the tester's monitor size.
#![cfg(feature = "test-harness")]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const GOLDEN: &str = "tests/golden/title_screen.png";
/// Fixed comparison resolution — the native capture (whatever the monitor is) is resized to this, so the
/// golden is monitor-independent and small. At this scale the title screen's live "watch feed" (small
/// moving units, light flicker) averages out against the static geometry/menu (measured SSIM ≈ 1.0
/// between two live frames), so a healthy render matches the golden almost exactly.
const W: u32 = 688;
const H: u32 = 288;
/// A healthy render scores ≈1.0; a real regression (a broken shader/material rendering pink, missing
/// geometry, or a layout shift) craters SSIM far below this. The margin below 1.0 absorbs the live
/// feed's motion and a possible transient VHS-glitch frame — and the best-of-N capture below removes most
/// of that risk anyway.
const THRESHOLD: f32 = 0.95;

fn to_gray(img: image::DynamicImage) -> Vec<f32> {
    img.resize_exact(W, H, image::imageops::FilterType::Triangle)
        .to_luma8()
        .pixels()
        .map(|p| p.0[0] as f32 / 255.0)
        .collect()
}

/// Drive one `devshot` capture and decode it, or `None` if no fresh screenshot appeared within `timeout`.
fn capture_once(timeout: Duration) -> Option<Vec<f32>> {
    let _ = std::fs::remove_file("screenshot.png");
    std::fs::write("screenshot.request", b"").ok()?;
    let start = Instant::now();
    while start.elapsed() < timeout {
        if Path::new("screenshot.png").exists() {
            std::thread::sleep(Duration::from_millis(400)); // let the GPU→PNG write finish
            if let Ok(img) = image::open("screenshot.png") {
                return Some(to_gray(img));
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

#[test]
#[ignore] // display-gated — see module doc.
fn title_screen_matches_golden() {
    let golden = to_gray(image::open(GOLDEN).expect("committed golden PNG must exist"));

    // Launch the real windowed game (inherits CWD = crate root, so it finds `assets/` and writes
    // `screenshot.*` here). Output silenced; a boot failure surfaces as "no screenshot" below.
    let mut child: Child = Command::new(env!("CARGO_BIN_EXE_foundation_vs_slop"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to launch the game binary");
    std::thread::sleep(Duration::from_secs(12)); // boot + first frames

    // Best of a few frames: a transient full-screen VHS-glitch frame can't fail an otherwise-healthy run.
    let mut best = 0.0f32;
    let mut captured = 0usize;
    for _ in 0..3 {
        if let Some(shot) = capture_once(Duration::from_secs(6)) {
            captured += 1;
            let s = foundation_vs_slop::visual_regression::ssim(&shot, &golden, W as usize, H as usize);
            if s > best {
                best = s;
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }

    // Always tear the game down before asserting, so a failure never leaks a process.
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file("screenshot.png");
    let _ = std::fs::remove_file("screenshot.request");

    assert!(
        captured > 0,
        "the windowed game produced no screenshot — no display available, or the window closed early \
         (this test needs a real window; it is #[ignore]d for exactly this reason)"
    );
    assert!(
        best >= THRESHOLD,
        "title-screen SSIM {best:.4} < {THRESHOLD} (best of {captured} frame(s) vs the golden) — a \
         rendering regression: a broken shader/material (pink), missing geometry, or a layout shift. \
         If the title art changed on purpose, regenerate tests/golden/title_screen.png (see module doc)."
    );
}
