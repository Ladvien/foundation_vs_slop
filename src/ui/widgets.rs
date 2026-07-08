//! Small, theme-aware UI building blocks.
//!
//! These are **bundle-returning** helpers (not spawner-taking functions), so callers compose them
//! with plain `commands.spawn(...)` / `parent.spawn(...)` and never have to name Bevy's child-
//! spawner type. Styling comes entirely from [`UiTheme`], keeping the CRT look in one place.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::theme::{FontAssets, UiTheme};

/// Marker for our themed menu buttons — used by [`style_menu_buttons`] to tint on hover.
#[derive(Component)]
pub struct MenuButton;

/// The visual + behavior bundle for a themed menu button. Spawn it, add a [`text`] child for the
/// label, and attach a `.observe(|_: On<Activate>, ..| ..)` handler:
/// ```ignore
/// parent.spawn(button_visual(&theme))
///     .with_children(|b| { b.spawn(text(&theme, &fonts, "New Run", theme.font_body)); })
///     .observe(|_: On<Activate>, mut next: ResMut<NextState<AppState>>| next.set(AppState::InGame));
/// ```
/// `bevy_ui_widgets::Button` (plugged in by `DefaultPlugins`) emits `Activate` on release; the
/// entity stays pickable (no `Pickable::IGNORE`) so clicks land.
pub fn button_visual(theme: &UiTheme) -> impl Bundle {
    (
        Button,
        Hovered::default(),
        MenuButton,
        Node {
            min_width: Val::Px(220.0),
            padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_sm)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(theme.panel),
        border_all(theme.panel_border),
    )
}

/// Hover tint for [`MenuButton`]s — runs only when a button's [`Hovered`] state flips.
pub fn style_menu_buttons(
    theme: Res<UiTheme>,
    mut buttons: Query<(&Hovered, &mut BackgroundColor), (With<MenuButton>, Changed<Hovered>)>,
) {
    for (hovered, mut bg) in &mut buttons {
        bg.0 = if hovered.0 {
            theme.panel_border.with_alpha(0.30)
        } else {
            theme.panel
        };
    }
}

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
