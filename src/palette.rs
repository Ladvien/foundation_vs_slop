//! Central world/gameplay color palette — the single source of truth for every non-UI color.
//!
//! Built from the SCP color language (`docs/lore/2026-07-12-scp-color-language.md`). The thesis:
//!
//! > **Desaturation = reality. Saturation = anomaly. Threat = luminosity, not hue.**
//! > The Foundation has no house palette — grayscale *is* its identity. Color belongs to the anomalous.
//!
//! So the *reality* layer (dungeon architecture, mundane props, agency gear) lives in near-monochrome
//! warm grays, and the *anomalous* layer (psi field, mushrooms, boss FX, the Psionic party member)
//! keeps its hue. Enemy damage-type tints follow the **GOC** Type/color matrix (§3 of the doc) — the
//! rival's color language, deliberately not Foundation vocabulary.
//!
//! UI colors are NOT here — they stay in [`crate::ui::theme`], which is themed separately. (That module
//! was a phosphor-green CRT terminal until 2026-07-29; it is now warm-neutral under `docs/ui.md` 1.3,
//! chroma capped by `MAX_UI_CHROMA`. This comment described the old palette for long enough to be
//! worth correcting rather than deleting.)
//!
//! Values are `pub const` (no per-frame theming needed for world colors), so routing through this
//! module is free. Systems reference `palette::FOO` instead of an inline `Color::srgb(...)`.

use bevy::color::LinearRgba;
use bevy::prelude::*;

// ─────────────────────────────────────────────────────────────────────────────
// Reality layer — near-monochrome, slightly warm (a photocopied-document look).
// ─────────────────────────────────────────────────────────────────────────────

/// Dungeon architecture / walls. Neutral-warm gray — reality reads as drab bureaucratic concrete,
/// not a colored set. (Was a cool blue-gray `srgb(0.28,0.28,0.36)`; warmed + neutralized here.)
pub const DUNGEON_STONE: Color = Color::srgb(0.30, 0.29, 0.27);

/// Speech/thought bubble & inert dialogue text tint — bone/paper gray.
pub const PAPER_GRAY: Color = Color::srgb(0.80, 0.80, 0.80);

// ─────────────────────────────────────────────────────────────────────────────
// The party — five distinct, fully-saturated outfit hues for instant unit readability.
// Index-matched to `squad_ai::role::RoleId::ALL` = [Gunman, Researcher, Psionic, Medic, Engineer].
// ─────────────────────────────────────────────────────────────────────────────

/// Combat specialist (Gunman) — red.
pub const OUTFIT_GUNMAN: Color = Color::srgb(0.85, 0.22, 0.20);
/// Researcher — blue.
pub const OUTFIT_RESEARCHER: Color = Color::srgb(0.22, 0.45, 0.90);
/// Psionic — green.
pub const OUTFIT_PSIONIC: Color = Color::srgb(0.25, 0.75, 0.32);
/// Medic — gold.
pub const OUTFIT_MEDIC: Color = Color::srgb(0.92, 0.76, 0.16);
/// Engineer — purple.
pub const OUTFIT_ENGINEER: Color = Color::srgb(0.66, 0.32, 0.82);

/// The five outfits in spawn/role order. Consumed by `squad.rs`.
pub const OUTFITS: [Color; 5] = [
    OUTFIT_GUNMAN,
    OUTFIT_RESEARCHER,
    OUTFIT_PSIONIC,
    OUTFIT_MEDIC,
    OUTFIT_ENGINEER,
];

/// Selection ring — the mark on the operatives the player's next order will move.
///
/// **Bright, warm-neutral, and deliberately not green.** It was `srgb(0.10, 1.00, 0.20)`: a chroma of
/// 0.90, the most saturated colour in the game. Two things made that wrong rather than merely loud:
///
///  1. `docs/lore/2026-07-12-scp-color-language.md` §7 — *"make color mean deviation, not danger"* —
///     and saturated green is specifically the GOC's Type Green (reality benders) and the four-phase
///     corruption arc. Painting *your own squad* in the anomaly colour inverts the whole grammar.
///  2. `docs/ui.md` §1.3 — selection is a status, and status rides **luminance, never hue**. The
///     roster chip's selection frame (`ui::hud::update_selection_marks`) already uses the UI accent
///     for exactly this; a ring in a different colour would make one selection read as two things.
///
/// The old doc-comment's reasoning — that it must stay legible however desaturated the world grades —
/// is *more* true now, not less, and brightness against a near-black floor is what delivers it. Hue
/// was never what made it visible.
pub const SELECTION_RING: Color = Color::srgb(0.96, 0.94, 0.88);

// ─────────────────────────────────────────────────────────────────────────────
// GOC damage-type matrix — the rival's color language for anomalous entities (§3).
// Magenta = Psionic, Blue = Thaumaturge, Yellow = Polymorph, Red = Regenerator,
// Gray(-green) = Reanimated, Green = Reality-bender. Used for enemy/creature tints & ichor.
// ─────────────────────────────────────────────────────────────────────────────

/// Type Magenta — psionic. **Is** the psi-vision "dread" hue (see [`GOC_MAGENTA_RGB`]).
pub const GOC_MAGENTA: Color = Color::srgb(GOC_MAGENTA_RGB[0], GOC_MAGENTA_RGB[1], GOC_MAGENTA_RGB[2]);
/// Type Blue — thaumaturge (cold UN blue). **No carrier yet** — the game has no thaumaturge entity, so
/// this is taxonomy waiting on content, not a wiring gap. Same status the config uses for
/// `photophilic_gain` ("toolkit; no carrier yet").
pub const GOC_BLUE: Color = Color::srgb(0.22, 0.45, 0.90);
/// Type Yellow — polymorph. **No carrier yet**, as [`GOC_BLUE`].
pub const GOC_YELLOW: Color = Color::srgb(0.92, 0.80, 0.20);
/// Type Red — regenerator. Hostile bolts / laser fire read as this; consumed via [`LASER_BOLT_BASE`].
pub const GOC_RED: Color = Color::srgb(1.00, 0.10, 0.08);
/// Type Gray — post-mortem reanimation (sickly desaturated gray-green). Consumed via [`CRAB_ICHOR`].
pub const GOC_GRAY_GREEN: Color = Color::srgb(0.20, 0.70, 0.15);
/// Type Green — reality bender. **Is** the psi-vision MEAT-trail hue (see [`GOC_GREEN_RGB`]).
pub const GOC_GREEN: Color = Color::srgb(GOC_GREEN_RGB[0], GOC_GREEN_RGB[1], GOC_GREEN_RGB[2]);

/// Shader-facing channel values for the two GOC hues a *uniform* needs, and the single source of
/// truth for them — [`GOC_MAGENTA`] / [`GOC_GREEN`] are built from these.
///
/// They exist because `psi_vision::PSI_GROUP_HUES` needs `[f32; 3]`, not `Color`, and previously
/// solved that by **hardcoding the same numbers a second time**: `[0.9, 0.15, 0.9]` and
/// `[0.15, 0.85, 0.5]`, byte-identical to the constants directly above them and referencing neither.
/// That is the failure this whole module exists to prevent — a palette value living in two places,
/// where changing one silently desynchronises the world from its own colour language.
/// `the_goc_matrix_has_no_duplicated_literals` pins it.
pub const GOC_MAGENTA_RGB: [f32; 3] = [0.90, 0.15, 0.90];
/// See [`GOC_MAGENTA_RGB`].
pub const GOC_GREEN_RGB: [f32; 3] = [0.15, 0.85, 0.50];

// ─────────────────────────────────────────────────────────────────────────────
// Concrete anomaly / FX colors (kept saturated — these ARE the anomalous things).
// Preserving the exact prior values so migration is behavior-neutral unless noted.
// ─────────────────────────────────────────────────────────────────────────────

/// Laser bolt body (base color) — GOC-red regenerator fire.
pub const LASER_BOLT_BASE: Color = GOC_RED;
/// Laser bolt emissive — red-dominant HDR so it reads as a vivid bolt.
pub const LASER_BOLT_EMISSIVE: LinearRgba = LinearRgba::rgb(7.0, 0.25, 0.1);
/// Laser scorch / hit tint — dark dried red.
pub const LASER_SCORCH: Color = Color::srgb(0.7, 0.05, 0.05);

/// Crab ichor (bright) — Type-Gray reanimated sickly green.
///
/// An **alias**, not a copy. It was independently spelled `Color::srgb(0.2, 0.7, 0.15)` — the same
/// three numbers as [`GOC_GRAY_GREEN`] directly above, with `crab/combat.rs` even commenting
/// "Type-Gray reanimated ichor" while referencing neither. Aliased the way [`LASER_BOLT_BASE`]
/// aliases [`GOC_RED`], so the crab's blood cannot drift away from the taxonomy it claims to follow.
pub const CRAB_ICHOR: Color = GOC_GRAY_GREEN;
/// Crab ichor (dulled variant).
pub const CRAB_ICHOR_DULL: Color = Color::srgb(0.35, 0.6, 0.15);

/// Boss lightning core — electric blue-white, HDR-bright.
pub const LIGHTNING_BASE: Color = Color::srgb(0.8, 0.9, 1.0);
/// Boss lightning emissive — HDR bolt.
pub const LIGHTNING_EMISSIVE: LinearRgba = LinearRgba::rgb(3.0, 6.0, 12.0);
/// Generic anomalous-hit scorch tint (enemy).
pub const ENEMY_SCORCH: Color = Color::srgb(0.7, 0.05, 0.05);

/// Blood pool base color.
pub const BLOOD_BASE: Color = Color::srgb(0.45, 0.0, 0.0);
/// Blood emissive floor (near-black, faint red).
pub const BLOOD_EMISSIVE: LinearRgba = LinearRgba::rgb(0.12, 0.0, 0.0);

/// Pale chitin ichor (parasite splatter).
pub const CHITIN_ICHOR: Color = Color::srgb(0.85, 0.80, 0.70);

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(c: Color) -> [f32; 3] {
        let s = c.to_srgba();
        [s.red, s.green, s.blue]
    }

    /// No GOC hue may be spelled a second time as a literal somewhere else.
    ///
    /// This module exists to be the single source of truth for every non-UI colour, and the GOC matrix
    /// was quietly failing at that in three places at once: `psi_vision::PSI_GROUP_HUES` hardcoded
    /// Magenta and Green as `[f32; 3]` literals, and `CRAB_ICHOR` re-spelled Gray-Green — each sitting
    /// a few lines from the constant it duplicated, each commented as if it referenced it. Every one
    /// would have survived a hue change to the constant and silently disagreed with it afterwards.
    ///
    /// Pins the aliasing rather than the values: change a GOC hue and its consumers follow, which is
    /// the whole point. Change only a consumer and this fails.
    #[test]
    fn the_goc_matrix_has_no_duplicated_literals() {
        assert_eq!(rgb(CRAB_ICHOR), rgb(GOC_GRAY_GREEN), "crab ichor must BE Type Gray, not match it");
        assert_eq!(rgb(LASER_BOLT_BASE), rgb(GOC_RED), "laser fire must BE Type Red, not match it");
        assert_eq!(rgb(GOC_MAGENTA), GOC_MAGENTA_RGB, "the Color and its shader array must agree");
        assert_eq!(rgb(GOC_GREEN), GOC_GREEN_RGB, "the Color and its shader array must agree");
        assert_eq!(
            crate::psi_vision::PSI_GROUP_HUES,
            [GOC_MAGENTA_RGB, [1.0, 0.28, 0.06], GOC_GREEN_RGB],
            "psi-vision's dread/meat hues must come from the GOC matrix, not be re-spelled"
        );
    }
}
