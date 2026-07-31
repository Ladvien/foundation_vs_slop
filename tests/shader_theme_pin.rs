//! **Source pin: colors duplicated across the Rust/WGSL boundary must agree.**
//!
//! GPU-free, no `App` — runs in the `cargo test` hard gate.
//!
//! `ui::theme` is the one place UI color lives — except where a shader needs the same color and
//! cannot read a Rust constant. `assets/shaders/health_bar.wgsl` hard-codes the theme's
//! `health_fill` because `HealthBarSettings` mirrors the uniform *layout* but carries no fill color,
//! so there is no channel to feed it through at runtime. Until someone builds that channel, this
//! test is the mechanism keeping the two literals equal — before it, the only enforcement was a
//! comment in the WGSL saying "matching `ui::theme`", in a repo whose lints exist precisely because
//! comments do not fail. (The cross-layer palette drift class is real here: a green health fill
//! shipped for weeks against a warm-neutral theme before the 2026-07 repaint caught it on a frame.)

use std::path::Path;

/// Extract three comma-separated f32s from `text` starting right after `anchor`.
/// Panics with a pointed message if the anchor or the parse is missing — a refactor that renames
/// either side must fail this test loudly, not skip it silently.
fn rgb_after<'t>(text: &'t str, anchor: &str, file: &str) -> [f32; 3] {
    let start = text
        .find(anchor)
        .unwrap_or_else(|| panic!("{file}: anchor {anchor:?} not found — if the constant moved or was renamed, update tests/shader_theme_pin.rs so the pin keeps pinning"));
    let rest = &text[start + anchor.len()..];
    let close = rest
        .find(')')
        .unwrap_or_else(|| panic!("{file}: no closing paren after {anchor:?}"));
    let nums: Vec<f32> = rest[..close]
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<f32>()
                .unwrap_or_else(|e| panic!("{file}: {anchor:?} argument {s:?} is not an f32: {e}"))
        })
        .collect();
    assert_eq!(nums.len(), 3, "{file}: expected exactly 3 components after {anchor:?}, got {nums:?}");
    [nums[0], nums[1], nums[2]]
}

#[test]
fn the_wgsl_health_fill_matches_the_theme() {
    let theme = std::fs::read_to_string(Path::new("src/ui/theme.rs"))
        .expect("read src/ui/theme.rs — is the working dir the crate root?");
    let wgsl = std::fs::read_to_string(Path::new("assets/shaders/health_bar.wgsl"))
        .expect("read assets/shaders/health_bar.wgsl");

    let rust = rgb_after(&theme, "health_fill: Color::srgb(", "src/ui/theme.rs");
    let shader = rgb_after(&wgsl, "let fill = vec3<f32>(", "assets/shaders/health_bar.wgsl");

    assert_eq!(
        rust, shader,
        "health-bar fill color has drifted across the Rust/WGSL boundary: \
         ui::theme says {rust:?}, health_bar.wgsl says {shader:?}. Change BOTH, or build the \
         real channel (add the fill to the uniform `HealthBarSettings` already mirrors) and \
         delete this pin."
    );
}
