//! Small, theme-aware UI building blocks.
//!
//! These are **bundle-returning** helpers (not spawner-taking functions), so callers compose them
//! with plain `commands.spawn(...)` / `parent.spawn(...)` and never have to name Bevy's child-
//! spawner type. Styling comes entirely from [`UiTheme`], keeping the CRT look in one place.

use bevy::prelude::*;

use super::theme::{FontAssets, UiTheme};

/// A text bundle in the theme's body font/color at `px` (before `theme.scale`).
pub fn text(theme: &UiTheme, fonts: &FontAssets, s: impl Into<String>, px: f32) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: FontSource::Handle(fonts.body.clone()),
            font_size: FontSize::Px(px * theme.scale),
            ..default()
        },
        TextColor(theme.text),
    )
}

/// A text bundle in an explicit color (e.g. accent/muted/danger).
pub fn text_colored(
    theme: &UiTheme,
    fonts: &FontAssets,
    s: impl Into<String>,
    px: f32,
    color: Color,
) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: FontSource::Handle(fonts.body.clone()),
            font_size: FontSize::Px(px * theme.scale),
            ..default()
        },
        TextColor(color),
    )
}

/// A bordered translucent panel container. `node` lets the caller set layout (size, flex, padding);
/// the theme supplies the fill + border. Spawn children into it with `.with_children`.
pub fn panel(theme: &UiTheme, node: Node) -> impl Bundle {
    (node, BackgroundColor(theme.panel), border_all(theme.panel_border))
}

/// The *back* (track) of a horizontal bar: a fixed-size box holding a [`bar_fill`] child.
pub fn bar_back(theme: &UiTheme, width_px: f32, height_px: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(width_px),
            height: Val::Px(height_px),
            ..default()
        },
        BackgroundColor(theme.health_back),
    )
}

/// The *fill* of a bar, sized to `frac` (0..1) of its parent width and tinted `color`.
pub fn bar_fill(frac: f32, color: Color) -> impl Bundle {
    (
        Node {
            width: Val::Percent(frac.clamp(0.0, 1.0) * 100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(color),
    )
}

/// A uniform border color on all four edges (Bevy 0.19 `BorderColor` is per-edge).
pub fn border_all(color: Color) -> BorderColor {
    BorderColor {
        top: color,
        right: color,
        bottom: color,
        left: color,
    }
}
