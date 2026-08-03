//! **The support-surface class vocabulary** — the one closed token table in the placement stack.
//!
//! Lived in `placement::furnish` until the Stage-0b workspace split. It had to move: it is a token
//! table and four pure functions, `manifest::validate_manifest` validates against it, and `manifest`
//! is engine-free — so the vocabulary could not stay behind the Bevy boundary that `furnish` is.
//!
//! `site::kit` validates against this same table, which is the point: growing the vocabulary is one
//! row here, never a second list that can drift.

use super::ir::Role;
use super::manifest::ManifestItem;

// Support-surface classes — the bitmask vocabulary that pairs a scatter prop with the *kind* of top it
// may rest on. A support piece `provides` the OR of the class bits for every token in its `surfaces`
// field (the *feature* axis — what a piece OFFERS — kept separate from `affordances`, the *service*
// axis, so a bed can afford "sleep" without doubling as a shelf); a scatter prop `requires` the bit for
// its `Role::Scatter { surface }` token, and rests only where `provides & requires != 0`. A typed
// support is a surface *feature*, not a generic shelf (Tutenel et al. 2010, "A Semantic Scene
// Description Language for Procedural Layout Solving", AIIDE; props attach to a specific support class in
// Infinigen Indoors, Raistrick et al. 2024, arXiv 2406.11824).
const SURFACE_SUPPORT: u32 = 1 << 0; // any support top (drawer/table/desk) — never a bed
const SURFACE_WORKTOP: u32 = 1 << 1; // a desk/table worktop only

/// The whole surface-class vocabulary, token → class bit — THE single source of truth. [`surface_bits`]
/// resolves through this table and `manifest::validate_manifest` walks it to reject unknown tokens at
/// load, so growing the vocabulary is one row here — never a second list that can drift.
pub const SURFACE_CLASSES: &[(&str, u32)] =
    &[("support", SURFACE_SUPPORT), ("worktop", SURFACE_WORKTOP)];

/// Map a support-surface token to its class bit. `support` = any support top; `worktop` = a desk/table.
/// An unrecognised token is `0` (matches nothing) — and `manifest::validate_manifest` rejects it at load
/// time, so a typo'd token errors at the door instead of silently dropping props at furnish time. Used
/// both for a support's provided classes and a scatter prop's required class.
pub fn surface_bits(token: &str) -> u32 {
    SURFACE_CLASSES
        .iter()
        .find(|(t, _)| *t == token)
        .map_or(0, |(_, b)| *b)
}

/// The surface classes a support piece provides — the OR of [`surface_bits`] over its `surfaces` field
/// (a desk with `surfaces: ["support", "worktop"]` provides both; a bed with no `surfaces` provides
/// nothing, so no prop ever rests on it). Sourced from `surfaces`, NOT `affordances`: what a piece
/// OFFERS (the feature axis) is separate from what it is FOR (the service axis) — Tutenel et al. 2010.
pub fn provided_surfaces(item: &ManifestItem) -> u32 {
    item.surfaces
        .iter()
        .map(|s| surface_bits(s))
        .fold(0, |acc, b| acc | b)
}

/// The surface class a scatter prop requires, from its `Role::Scatter { surface }` token. A non-Scatter
/// role (never reached in Pass 4) requires nothing.
pub fn required_surface(item: &ManifestItem) -> u32 {
    match &item.role {
        Role::Scatter { surface } => surface_bits(surface),
        _ => 0,
    }
}

