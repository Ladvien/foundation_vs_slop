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

/// Regenerate the committed golden from a `screenshot.png` sitting in the crate root.
///
/// The module doc above described this procedure in prose and nothing implemented it, which left the
/// riskiest part to hand: the golden **must** be produced by the same resize + grayscale path
/// [`to_gray`] uses for the live capture, or the comparison is against a differently-filtered image
/// and the threshold means nothing. Sharing [`W`]/[`H`]/`FilterType::Triangle` with the comparison is
/// the whole point of it being code.
///
/// `#[ignore]`d and never run by CI: re-pinning a golden is a deliberate, human-reviewed act
/// (`TESTING.md` — "never auto-approve a diff"). Capture a clean title frame first, **look at it**,
/// then:
///   `cargo test --features test-harness --test visual_capture -- --ignored regenerate_golden`
#[test]
#[ignore] // regeneration tool, not a check.
fn regenerate_golden_from_screenshot() {
    let src = image::open("screenshot.png").expect(
        "put a freshly captured screenshot.png in the crate root first (touch screenshot.request \
         while the windowed game runs)",
    );
    src.resize_exact(W, H, image::imageops::FilterType::Triangle)
        .to_luma8()
        .save(GOLDEN)
        .expect("failed to write the golden");
    eprintln!("re-pinned {GOLDEN} at {W}x{H} grayscale from screenshot.png");
}

/// **Re-pinned 2026-07-29** after the UI/controls pass, having inspected the live render.
///
/// Two changes are in the new golden, and only the second is from that pass:
///
///  1. **The golden had already been stale for ~90 commits** and this test had been failing unnoticed
///     the whole time (it is `#[ignore]`d): measured SSIM 0.8873 *before* any of the 2026-07-29 work,
///     because 43 file-touches to `src/{dungeon,world,health,light,mycelia,placement}` had landed
///     since `ad098e5` — the dungeon behind the title card is simply a different world now.
///  2. The **palette desaturation** (`docs/ui.md` §1.3). Title text, menu, worldspace health bars and
///     selection rings all moved from phosphor/saturated green to warm neutral.
///
/// The capture that justified re-pinning also *found a bug*, which is the argument for having looked
/// rather than just re-pinning: the UI had desaturated but `health_bar.wgsl` and
/// `palette::SELECTION_RING` had not, so the bars and rings were left as the most saturated things on
/// screen — chroma 0.55 and 0.90 against a HUD that had just dropped to 0.07. Every unit test passed.
/// `health::the_health_bar_fill_matches_the_theme_and_stays_desaturated` and
/// `health::the_selection_ring_is_bright_rather_than_green` now guard both.
///
/// Note also that the subtitle's em-dash in `"// SCP-9191 CONTAINMENT SITE — WATCH FEED"` renders
/// rather than tofu-ing, because `ui::theme::load_fonts` loads the full
/// `assets/fonts/FiraMono-Regular.ttf` instead of resolving to Bevy's embedded 95-codepoint
/// `FiraMono-subset.ttf`. That shifts a line of centred text; it does not move geometry.
///
/// **Re-pinned 2026-07-30** after the relight (HDR + `Bloom`, an irradiance environment map replacing
/// the flat ambient, directional cascade shadows), surface normal/ORM maps on the dungeon, and the
/// Backrooms/Concrete biome split. Measured SSIM against the previous golden: **0.5403** — a real,
/// intended change to the whole render, not a regression.
///
/// **Two traps cost real time here; both are avoidable.**
///
///  1. `cargo test ... -- --ignored` runs *both* ignored tests in this file, and
///     [`regenerate_golden_from_screenshot`] silently re-pins from whatever `screenshot.png` happens to
///     be lying in the crate root. Run them **by name**, never together, or the comparison grades
///     itself against a golden the same invocation just overwrote.
///  2. A stale game process will hand you the wrong scene. `pkill -f target/debug/foundation_vs_slop`
///     matches its own wrapper shell and kills that instead of the game (`pkill -x foundation_vs_s`
///     works — the name is truncated to 15 chars). A surviving `FVS_RESEARCH_ROOM=1` instance
///     produced a *dungeon* capture that was then pinned as the "title screen"; the resulting 0.53
///     scores looked exactly like a non-reproducible scene and were nothing of the kind. **Open the
///     PNG and confirm you are looking at the title card before re-pinning.**
///
/// To re-pin again, use [`regenerate_golden_from_screenshot`] — and look at the frame first.
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
