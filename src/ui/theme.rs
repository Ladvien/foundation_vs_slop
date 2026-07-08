//! Design tokens + fonts for the **surveillance-terminal / CRT** UI look.
//!
//! Everything visual routes through [`UiTheme`] so the aesthetic (phosphor green on near-black,
//! bone-white text) and spacing live in one place — swapping the palette or scaling the whole HUD
//! (accessibility text-scale, HUD density) is a one-resource change.
//!
//! **Fonts:** the body/display handles currently resolve to Bevy's embedded `default_font`
//! (Fira Mono — a fitting terminal face), so text renders with no shipped asset. Drop an OFL/CC0
//! display face into `assets/fonts/` and load it in [`load_fonts`] to upgrade the numerals.

use bevy::prelude::*;

/// `GlobalZIndex` layers so overlays stack deterministically. The blood-lens overlay is `50`
/// (`blood_lens.rs`), so the HUD sits below it and menus well above it.
pub const Z_HUD: i32 = 10;
pub const Z_MENU_DIM: i32 = 90;
pub const Z_MENU: i32 = 100;

/// Loaded UI font handles. Both default to Bevy's embedded font until a dedicated face is added.
#[derive(Resource, Default, Clone)]
pub struct FontAssets {
    pub body: Handle<Font>,
    pub display: Handle<Font>,
}

/// Central design tokens. `scale` multiplies font sizes + spacing (driven later by the HUD-density
/// / accessibility text-scale settings); default `1.0`.
#[derive(Resource, Clone)]
pub struct UiTheme {
    pub bg: Color,
    pub panel: Color,
    pub panel_border: Color,
    /// Phosphor accent (green) — the terminal's primary UI ink.
    pub accent: Color,
    pub danger: Color,
    pub warn: Color,
    /// Bone-white primary text.
    pub text: Color,
    pub text_muted: Color,
    pub health_fill: Color,
    pub health_back: Color,
    pub space_xs: f32,
    pub space_sm: f32,
    pub space_md: f32,
    pub space_lg: f32,
    pub radius: f32,
    pub font_body: f32,
    pub font_title: f32,
    pub scale: f32,
}

impl Default for UiTheme {
    fn default() -> Self {
        Self {
            bg: Color::srgba(0.01, 0.03, 0.02, 0.86),
            panel: Color::srgba(0.02, 0.06, 0.04, 0.74),
            panel_border: Color::srgba(0.35, 0.85, 0.45, 0.55),
            accent: Color::srgb(0.55, 1.0, 0.62),
            danger: Color::srgb(0.95, 0.28, 0.22),
            warn: Color::srgb(0.98, 0.78, 0.28),
            text: Color::srgb(0.86, 0.92, 0.86),
            text_muted: Color::srgba(0.70, 0.82, 0.72, 0.75),
            health_fill: Color::srgb(0.45, 0.95, 0.5),
            health_back: Color::srgba(0.0, 0.0, 0.0, 0.6),
            space_xs: 3.0,
            space_sm: 6.0,
            space_md: 12.0,
            space_lg: 20.0,
            radius: 3.0,
            font_body: 15.0,
            font_title: 44.0,
            scale: 1.0,
        }
    }
}

pub struct UiThemePlugin;

impl Plugin for UiThemePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiTheme>()
            .init_resource::<FontAssets>()
            .add_systems(Startup, load_fonts);
    }
}

/// Populate [`FontAssets`]. Both handles resolve to the embedded default font today; to upgrade,
/// `assets.load("fonts/<face>.ttf")` here and let [`crate::ui::boot`] gate `Boot → Title` on it.
fn load_fonts(mut fonts: ResMut<FontAssets>) {
    // `Handle::<Font>::default()` maps to Bevy's embedded `default_font` (Fira Mono).
    fonts.body = Handle::default();
    fonts.display = Handle::default();
}
