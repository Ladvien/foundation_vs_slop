//! Design tokens + fonts for the **surveillance-terminal / CRT** UI look.
//!
//! Everything visual routes through [`UiTheme`] so the aesthetic and spacing live in one place.
//!
//! # The palette is near-monochrome, and that is a rule, not a taste
//!
//! `docs/lore/2026-07-12-scp-color-language.md` sets two constraints this file now obeys. §6's core
//! rule is **"Desaturation = reality. Saturation = anomaly"**, with baseline reality described as
//! *"near-monochrome and slightly warm — like a photocopied document"*; §7's guardrail is **"Don't:
//! Give the Foundation a house palette. It doesn't have one and shouldn't."** The UI used to be
//! phosphor green on near-black, which was both a house palette and the most saturated surface on
//! screen — so the game's loudest colour signal was spent on chrome.
//!
//! Two independent arguments say that was the wrong place to spend it:
//!
//! - *In-fiction*: §7 again — **"Make color mean deviation, not danger."** A colour reserved for
//!   chrome cannot mean anything.
//! - *Perceptual*: Wolfe, *Guided Search 6.0* (Psychon Bull Rev 2021, DOI 10.3758/s13423-020-01859-9)
//!   — only a small set of features guide attention at all, and colour is one of them. Spending
//!   saturation on a permanent background element burns the channel that could have pulled the eye
//!   to a threat. Rosenholtz 2016 (DOI 10.1146/annurev-vision-082114-035733) adds the periphery half:
//!   a screen-edge element is encoded as summary statistics — a colour blob — so its hue is doing
//!   *more* work out there, not less.
//!
//! So every token below is warm-neutral (`red >= blue`, chroma under [`MAX_UI_CHROMA`]) and
//! separation between them rides on **luminance**, the same encoding rule [`Hazard`] and
//! `rows::Emphasis` already follow. [`UiTheme::anomaly`] holds the vacated saturation, reserved.
//! Both invariants are unit-tested at the bottom of this file.
//!
//! **Threat never encodes as hue.** The SCP ACS Disruption scale is a *luminosity* scale — Dark →
//! Vlam (candle) → Keneq (campfire) → Ekhi (sun) → Amida (the screen cannot hold it) — and
//! `docs/lore/2026-07-12-scp-color-language.md` §6 says to use it as one: *"Use the ACS luminosity
//! scale, not a color scale."* [`Hazard`] implements that, and pairs it with a **glyph** so
//! severity survives in grayscale, at low contrast, and for the ~8% of men with a red-green colour
//! vision deficiency. A green→amber→red ramp would have been the worst possible choice on all
//! three counts. See `docs/ui.md` §1.3.
//!
//! **Two scale knobs, both Bevy-native, one path each** (`docs/ui.md` §2):
//! - `accessibility.text_scale` → [`bevy::text::RemSize`]. Every label is emitted as
//!   `FontSize::Rem(px / REM_BASE)`, so raising the resource scales all text proportionally.
//! - `hud.hud_scale` → [`UiScale`], which scales every `Val::Px` (padding, gaps, bar sizes) and
//!   leaves `Percent`/`Vw`/`Vh` alone — so panels keep their proportions while their chrome grows.
//!
//! There is deliberately **no third scale field on this struct**. A hand-rolled multiplier that
//! only reached `font_size` (which is what `UiTheme::scale` used to be) grows the glyphs inside
//! boxes that stayed the same size, and text overflows the chrome.

use bevy::prelude::*;
use bevy::text::RemSize;

use crate::settings::{AccessibilitySettings, HudSettings};

/// `GlobalZIndex` layers, lowest first, so overlays stack deterministically and the order is
/// readable in one place.
///
/// Before this was named, seven panels spelled their layer as the ad-hoc `Z_MENU - 1` (= 99), which
/// put them **above** [`Z_MENU_DIM`]: opening the pause menu dimmed the world but left the
/// containment readout, verb bar, briefing, research, Site, records and requisition panels burning
/// at full brightness on top of the scrim. Panels belong under the scrim, which is what [`Z_PANEL`]
/// encodes.
pub const Z_HUD: i32 = 10;
/// Content panels (containment, verbs, briefing, research, Site, records, requisition).
pub const Z_PANEL: i32 = 20;
/// Blood-on-lens splatter — owned by `crate::blood_lens`, named here so the stack reads in order.
/// It is *lens* dirt, so it sits over the HUD and under the menus.
pub const Z_BLOOD_LENS: i32 = 50;
pub const Z_MENU_DIM: i32 = 90;
pub const Z_MENU: i32 = 100;

/// The rem base the `font_*` tokens below are authored against.
///
/// Text is emitted as `FontSize::Rem(px / REM_BASE)` rather than `Px`, so the [`RemSize`] resource
/// is the single lever for text scale. Matches Bevy's own `RemSize` default, so an unscaled build
/// renders the tokens at exactly their stated pixel size.
pub const REM_BASE: f32 = 20.0;

/// Loaded UI font handles.
///
/// Both resolve to `assets/fonts/FiraMono-Regular.ttf`. **This must be the on-disk face, not
/// `Handle::default()`** — Bevy's embedded `default_font` is `FiraMono-subset.ttf`, a **95
/// codepoint** subset that is essentially bare ASCII. Every non-ASCII character in the UI copy
/// renders as tofu under it, which was 54 live sites across 10 glyphs, including the `▓░` challenge
/// meters that are the entire visual content of the expedition briefing. The shipped face carries
/// 1350 codepoints and draws all of them.
///
/// Anything added to [`glyph`] must be checked against this face, not assumed.
#[derive(Resource, Default, Clone)]
pub struct FontAssets {
    pub body: Handle<Font>,
    pub display: Handle<Font>,
}

/// Glyphs used as the **redundant, non-colour channel** on every status readout.
///
/// Each is verified present in `assets/fonts/FiraMono-Regular.ttf`. Notable absentees that look
/// like obvious choices and are **not** in the face: `✓` (U+2713), `▶`/`▸` (U+25B6/25B8), `⚠`
/// (U+26A0), `★` (U+2605). Do not reach for them.
pub mod glyph {
    /// A satisfied condition: settled, recedes.
    pub const MET: &str = "·";
    /// An unsatisfied condition: demands an action. ASCII, so it can never tofu.
    pub const UNMET: &str = "!";
    /// The item the player is currently on.
    pub const CURRENT: &str = "»";
    /// Not yet available.
    pub const LOCKED: &str = "▫";
    /// Complete.
    pub const DONE: &str = "■";
    /// Bar track / meter, empty and full cells.
    pub const METER_EMPTY: &str = "░";
    pub const METER_FULL: &str = "▓";
}

/// SCP **ACS Disruption class**, used as this UI's single threat scale.
///
/// Ordered by how much light is getting out, which is the in-fiction meaning and also the only
/// encoding that survives a colour-vision deficiency. [`Hazard::ink`] returns the *same hue* at
/// five luminances; [`Hazard::glyph`] is the redundant channel; [`Hazard::label`] is the third.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Hazard {
    /// Contained. No light escaping.
    Dark,
    /// Candle.
    Vlam,
    /// Campfire.
    Keneq,
    /// Sun.
    Ekhi,
    /// The screen cannot hold it. Reserved — see the lore doc's open question #3.
    Amida,
}

impl Hazard {
    /// 0..1 along the ramp, for driving a bar fill or an alpha.
    pub fn intensity(self) -> f32 {
        match self {
            Hazard::Dark => 0.0,
            Hazard::Vlam => 0.25,
            Hazard::Keneq => 0.5,
            Hazard::Ekhi => 0.75,
            Hazard::Amida => 1.0,
        }
    }

    /// The severity mark. Monotonically heavier in ink, so the ordering reads with colour removed.
    pub fn glyph(self) -> &'static str {
        match self {
            Hazard::Dark => "·",
            Hazard::Vlam => "▪",
            Hazard::Keneq => "■",
            Hazard::Ekhi => "▲",
            Hazard::Amida => "█",
        }
    }

    /// The tier's in-fiction name, for the readout's third channel.
    pub fn label(self) -> &'static str {
        match self {
            Hazard::Dark => "DARK",
            Hazard::Vlam => "VLAM",
            Hazard::Keneq => "KENEQ",
            Hazard::Ekhi => "EKHI",
            Hazard::Amida => "AMIDA",
        }
    }
}

/// Chroma ceiling for every token that describes **baseline reality** — the machine-checkable form
/// of "the Foundation has no house palette" (see the module note).
///
/// Chroma is `max(r,g,b) - min(r,g,b)` in sRGB: a cheap saturation proxy that needs no colour-space
/// conversion and is monotone in the thing being bounded. [`UiTheme::anomaly`], [`UiTheme::danger`]
/// and [`UiTheme::warn`] are the three deliberate exemptions — they do not describe reality.
pub const MAX_UI_CHROMA: f32 = 0.12;

/// Central design tokens.
#[derive(Resource, Clone)]
pub struct UiTheme {
    pub bg: Color,
    pub panel: Color,
    pub panel_border: Color,
    /// The terminal's primary ink — the **brightest** warm-neutral in the palette, so it separates
    /// from [`UiTheme::text`] by luminance rather than by hue.
    pub accent: Color,
    /// **Reserved saturation.** The one place the UI is allowed to be colourful, because colour here
    /// means *deviation* (`docs/lore/2026-07-12-scp-color-language.md` §7), not danger and not
    /// branding. This is the phosphor green the chrome used to be, now carrying meaning instead of
    /// decorating. Nothing consumes it yet; the lore doc's Type system (§2) implies the hue may
    /// eventually vary by anomaly class, so treat it as the *default* anomaly ink, not the only one.
    pub anomaly: Color,
    /// Reserved for **destructive or irreversible** actions, not for threat (see [`Hazard`]).
    pub danger: Color,
    /// Reserved for **cautionary copy**, not for threat.
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
    /// Body text size in px, at `RemSize == REM_BASE`.
    pub font_body: f32,
    /// Display text size in px, at `RemSize == REM_BASE`.
    pub font_title: f32,
}

impl Default for UiTheme {
    fn default() -> Self {
        Self {
            // Warm-neutral throughout: `red >= green >= blue` by a hair, chroma well under
            // `MAX_UI_CHROMA`. Separation is luminance — accent (brightest) > text > text_muted.
            bg: Color::srgba(0.030, 0.028, 0.024, 0.86),
            panel: Color::srgba(0.058, 0.054, 0.047, 0.74),
            panel_border: Color::srgba(0.60, 0.58, 0.53, 0.55),
            accent: Color::srgb(0.95, 0.93, 0.88),
            // The vacated phosphor green. Exempt from the chroma ceiling by design — see the field.
            anomaly: Color::srgb(0.55, 1.0, 0.62),
            danger: Color::srgb(0.95, 0.28, 0.22),
            warn: Color::srgb(0.98, 0.78, 0.28),
            text: Color::srgb(0.82, 0.80, 0.76),
            text_muted: Color::srgba(0.62, 0.60, 0.57, 0.78),
            health_fill: Color::srgb(0.80, 0.78, 0.73),
            health_back: Color::srgba(0.0, 0.0, 0.0, 0.6),
            space_xs: 3.0,
            space_sm: 6.0,
            space_md: 12.0,
            space_lg: 20.0,
            radius: 3.0,
            font_body: 15.0,
            font_title: 44.0,
        }
    }
}

impl UiTheme {
    /// The ink for a hazard tier: one hue, five luminances, brightening to near-white at `Amida`
    /// ("the screen cannot hold it"). Never a hue ramp.
    pub fn hazard_ink(&self, h: Hazard) -> Color {
        let t = h.intensity();
        let base = self.accent.to_linear();
        // Lift luminance across the ramp, then wash toward white at the top so the last tier reads
        // as overexposure rather than as "more ink". Derived from `accent`, so the ramp desaturated
        // with the rest of the palette — which it always should have been: the ACS Disruption scale
        // is *how much light is getting out*, and light has no hue.
        let gain = 0.35 + 0.95 * t;
        let wash = (t - 0.75).max(0.0) * 4.0; // 0 until Ekhi, 1 at Amida
        let mix = |c: f32| (c * gain) * (1.0 - wash) + wash * 1.6;
        Color::LinearRgba(LinearRgba::new(
            mix(base.red),
            mix(base.green),
            mix(base.blue),
            1.0,
        ))
    }

    /// Ink for a row's emphasis. Same hue, three luminances — an unmet clause is *brighter*, not a
    /// different colour, so the eye lands on it without depending on hue discrimination.
    pub fn emphasis_ink(&self, muted: bool, alert: bool) -> Color {
        if alert {
            self.text
        } else if muted {
            self.text_muted
        } else {
            // Sits between `text` and `text_muted` in luminance and shares their chromaticity — a
            // third rung on one ladder, not a third colour. Pinned by
            // `rows::an_unmet_row_is_louder_than_a_met_one_without_changing_hue`.
            Color::srgba(0.72, 0.70, 0.66, 0.92)
        }
    }
}

pub struct UiThemePlugin;

impl Plugin for UiThemePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiTheme>()
            .init_resource::<FontAssets>()
            // Claimed here rather than relied on from `DefaultPlugins`: in Bevy 0.19 a missing
            // `Res<T>` **panics** the system rather than skipping it, and `UiPlugin` is added to a
            // bare `App` by the UI-liveness test (which has no `TextPlugin`/`UiPlugin` from Bevy to
            // seed these). `init_resource` is idempotent, so claiming them is free.
            .init_resource::<RemSize>()
            .init_resource::<UiScale>()
            .add_systems(Startup, load_fonts)
            .add_systems(Update, (apply_text_scale, apply_hud_scale));
    }
}

/// Populate [`FontAssets`] from the on-disk face.
///
/// `crate::ui::boot` gates `Boot → Title` on `body` being loaded, so no frame renders text before
/// the atlas is ready.
/// `pub(crate)` so a dev overlay outside `ui::` can order its own `Startup` spawn after the face is
/// loaded, rather than reaching for `Handle::default()` (the tofu-prone 95-codepoint subset).
pub(crate) fn load_fonts(assets: Res<AssetServer>, mut fonts: ResMut<FontAssets>) {
    // One face for both roles today. `display` stays a distinct handle so a display face can be
    // dropped in without touching any call site.
    let face: Handle<Font> = assets.load("fonts/FiraMono-Regular.ttf");
    fonts.body = face.clone();
    fonts.display = face;
}

/// `accessibility.text_scale` → [`RemSize`]. Sole writer.
fn apply_text_scale(acc: Res<AccessibilitySettings>, mut rem: ResMut<RemSize>) {
    if !acc.is_changed() {
        return;
    }
    let want = REM_BASE * acc.text_scale.clamp(0.75, 1.5);
    if rem.0 != want {
        rem.0 = want;
    }
}

/// `hud.hud_scale` → [`UiScale`]. Sole writer.
fn apply_hud_scale(hud: Res<HudSettings>, mut scale: ResMut<UiScale>) {
    if !hud.is_changed() {
        return;
    }
    let want = hud.hud_scale.clamp(0.75, 1.5);
    if scale.0 != want {
        scale.0 = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hazard_ramp_is_monotonic_in_luminance() {
        // The whole point of the ACS scale: severity reads as "how much light is getting out", so a
        // higher tier must never be dimmer than a lower one. If this fails, threat has stopped
        // being legible in grayscale and for red-green CVD players.
        let theme = UiTheme::default();
        let tiers = [
            Hazard::Dark,
            Hazard::Vlam,
            Hazard::Keneq,
            Hazard::Ekhi,
            Hazard::Amida,
        ];
        let lum = |h: Hazard| {
            let c = theme.hazard_ink(h).to_linear();
            // Rec. 709 relative luminance.
            0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue
        };
        for pair in tiers.windows(2) {
            let (lo, hi) = (pair[0], pair[1]);
            assert!(
                lum(hi) > lum(lo),
                "{:?} ({:.3}) must be brighter than {:?} ({:.3})",
                hi,
                lum(hi),
                lo,
                lum(lo)
            );
        }
    }

    /// sRGB chroma proxy — see [`MAX_UI_CHROMA`].
    fn chroma(c: Color) -> f32 {
        let c = c.to_srgba();
        let hi = c.red.max(c.green).max(c.blue);
        let lo = c.red.min(c.green).min(c.blue);
        hi - lo
    }

    fn relative_luminance(c: Color) -> f32 {
        let c = c.to_linear();
        // Rec. 709.
        0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue
    }

    #[test]
    fn the_foundation_has_no_house_palette() {
        // `docs/lore/2026-07-12-scp-color-language.md` §7, made machine-checkable. Every token that
        // describes BASELINE REALITY stays near-monochrome, so saturation is free to mean anomaly
        // (§6's core rule). This is the test that would have failed on the phosphor-green UI: its
        // accent had a chroma of 0.45.
        let t = UiTheme::default();
        for (name, c) in [
            ("bg", t.bg),
            ("panel", t.panel),
            ("panel_border", t.panel_border),
            ("accent", t.accent),
            ("text", t.text),
            ("text_muted", t.text_muted),
            ("health_fill", t.health_fill),
            ("emphasis/normal", t.emphasis_ink(false, false)),
        ] {
            assert!(
                chroma(c) <= MAX_UI_CHROMA,
                "{name} has chroma {:.3} — reality is desaturated (max {MAX_UI_CHROMA})",
                chroma(c)
            );
        }

        // The three deliberate exemptions. Asserted *loud* rather than merely omitted, so deleting
        // one from the list above can never quietly pass as "it was always neutral".
        for (name, c) in [("anomaly", t.anomaly), ("danger", t.danger), ("warn", t.warn)] {
            assert!(
                chroma(c) > MAX_UI_CHROMA,
                "{name} is the exemption — if it desaturates it has stopped carrying its meaning"
            );
        }
    }

    #[test]
    fn baseline_reality_is_slightly_warm() {
        // The other half of §6's description: "near-monochrome and slightly warm — like a
        // photocopied document". Chroma alone would accept a cold blue-gray, which reads as a
        // computer display rather than as paper.
        let t = UiTheme::default();
        for (name, c) in [
            ("bg", t.bg),
            ("panel", t.panel),
            ("panel_border", t.panel_border),
            ("accent", t.accent),
            ("text", t.text),
            ("text_muted", t.text_muted),
            ("health_fill", t.health_fill),
        ] {
            let c = c.to_srgba();
            assert!(
                c.red >= c.blue,
                "{name} is cool ({:.3}R vs {:.3}B) — paper is warm, screens are cold",
                c.red,
                c.blue
            );
        }
    }

    #[test]
    fn the_ink_ladder_separates_by_luminance_alone() {
        // With hue gone, accent/text/muted have nothing BUT luminance to tell them apart. If two
        // rungs converge the HUD loses a channel silently — the panel still renders, it just stops
        // saying anything. (`rows.rs` pins the same property for `Emphasis`; this pins the tokens
        // those rungs are built from.)
        let t = UiTheme::default();
        let (accent, text, muted) = (
            relative_luminance(t.accent),
            relative_luminance(t.text),
            relative_luminance(t.text_muted),
        );
        assert!(accent > text, "accent {accent:.3} must outrank text {text:.3}");
        assert!(text > muted, "text {text:.3} must outrank muted {muted:.3}");
        // A step the eye can actually resolve, not a rounding difference.
        assert!(accent - text > 0.05, "accent/text step is {:.3} — too fine to read", accent - text);
        assert!(text - muted > 0.05, "text/muted step is {:.3} — too fine to read", text - muted);
    }

    #[test]
    fn every_hazard_tier_has_a_distinct_glyph_and_label() {
        // The glyph and the label are the two non-colour channels. If two tiers shared either, the
        // redundancy would be a lie and severity would be hue-only again for someone who cannot
        // separate the luminances.
        let tiers = [
            Hazard::Dark,
            Hazard::Vlam,
            Hazard::Keneq,
            Hazard::Ekhi,
            Hazard::Amida,
        ];
        for (i, a) in tiers.iter().enumerate() {
            for b in &tiers[i + 1..] {
                assert_ne!(a.glyph(), b.glyph(), "{a:?} and {b:?} share a glyph");
                assert_ne!(a.label(), b.label(), "{a:?} and {b:?} share a label");
            }
        }
    }

    #[test]
    fn hazard_intensity_spans_the_full_range_in_order() {
        assert_eq!(Hazard::Dark.intensity(), 0.0);
        assert_eq!(Hazard::Amida.intensity(), 1.0);
        assert!(Hazard::Vlam.intensity() < Hazard::Keneq.intensity());
        assert!(Hazard::Keneq.intensity() < Hazard::Ekhi.intensity());
    }

    #[test]
    fn the_layer_stack_is_ordered_and_panels_sit_under_the_menu_scrim() {
        // Pins the bug this constant exists to fix: panels used to spell their layer `Z_MENU - 1`
        // (99), which rendered them ABOVE the pause scrim (90) — the world dimmed and the panels
        // did not.
        assert!(Z_HUD < Z_PANEL, "HUD chrome sits under content panels");
        assert!(Z_PANEL < Z_BLOOD_LENS, "lens dirt is in front of the HUD");
        assert!(
            Z_PANEL < Z_MENU_DIM,
            "the pause scrim must cover content panels, not sit under them"
        );
        assert!(Z_MENU_DIM < Z_MENU, "menus draw over their own scrim");
    }
}
