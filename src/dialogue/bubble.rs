//! World-space speech/thought balloons — the game's half of `bevy_speech_bubbles`.
//!
//! The rasterizer, the balloon shapes, the components and both systems moved to that crate: none of
//! them knew anything about this game beyond which camera to face, and that is now a type parameter.
//!
//! What stays here is the one thing a library must not assume — **where the font lives**. `ab_glyph`
//! needs raw bytes from a real file, Bevy's embedded default-font bytes are not exposed, and a crate
//! that hardcoded `assets/fonts/…` would be hardcoding this project's layout. So the asset resource is
//! built here, from this project's path, and inserted for the crate's systems to read.

use bevy::prelude::*;

use ab_glyph::FontArc;

pub use bevy_speech_bubbles::{
    build_bubble, dwell_secs, expire_bubbles, track_bubbles, Bubble, BubbleAssets, BubbleStyle,
    BubbleTtl, RenderedBubble, BUBBLE_ANCHOR_Y,
};

/// TTF shipped with the game (OFL, Fira Mono) — a terminal face fitting the CRT theme.
const FONT_PATH: &str = "assets/fonts/FiraMono-Regular.ttf";

/// Load the dialogue font and the shared quad, and hand them to the bubble crate.
///
/// Panics loudly on a missing font, deliberately: this is startup, the font is a shipped asset, and a
/// bubble system that silently drew nothing would be a much worse failure to diagnose than a message
/// naming the file at boot.
pub fn setup_bubble_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    let bytes = std::fs::read(FONT_PATH).unwrap_or_else(|e| panic!("dialogue font {FONT_PATH}: {e}"));
    let font =
        FontArc::try_from_vec(bytes).unwrap_or_else(|e| panic!("dialogue font {FONT_PATH}: {e}"));
    commands.insert_resource(BubbleAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        font,
    });
}
