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
//! UI colors are NOT here — they stay in [`crate::ui::theme`] (the phosphor-green CRT terminal), which
//! is a deliberate diegetic SCiPNET-console look and is themed separately.
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

/// Selection ring — deliberately bright cyan-green, high-contrast against both drab gear and the
/// near-monochrome floor, so unit selection stays legible no matter how desaturated the world grades.
pub const SELECTION_RING: Color = Color::srgb(0.10, 1.00, 0.20);

// ─────────────────────────────────────────────────────────────────────────────
// GOC damage-type matrix — the rival's color language for anomalous entities (§3).
// Magenta = Psionic, Blue = Thaumaturge, Yellow = Polymorph, Red = Regenerator,
// Gray(-green) = Reanimated, Green = Reality-bender. Used for enemy/creature tints & ichor.
// ─────────────────────────────────────────────────────────────────────────────

/// Type Magenta — psionic. Matches the psi-vision "dread" hue.
pub const GOC_MAGENTA: Color = Color::srgb(0.90, 0.15, 0.90);
/// Type Blue — thaumaturge (cold UN blue).
pub const GOC_BLUE: Color = Color::srgb(0.22, 0.45, 0.90);
/// Type Yellow — polymorph.
pub const GOC_YELLOW: Color = Color::srgb(0.92, 0.80, 0.20);
/// Type Red — regenerator. Hostile bolts / laser fire read as this.
pub const GOC_RED: Color = Color::srgb(1.00, 0.10, 0.08);
/// Type Gray — post-mortem reanimation (sickly desaturated gray-green). Crab ichor.
pub const GOC_GRAY_GREEN: Color = Color::srgb(0.20, 0.70, 0.15);
/// Type Green — reality bender (reserve for a boss-tier).
pub const GOC_GREEN: Color = Color::srgb(0.15, 0.85, 0.50);

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
pub const CRAB_ICHOR: Color = Color::srgb(0.2, 0.7, 0.15);
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
