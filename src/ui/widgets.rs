//! Small, theme-aware UI building blocks.
//!
//! These are **bundle-returning** helpers (not spawner-taking functions), so callers compose them
//! with plain `commands.spawn(...)` / `parent.spawn(...)` and never have to name Bevy's child-
//! spawner type. Styling comes entirely from [`UiTheme`], keeping the CRT look in one place.

use bevy::input_focus::tab_navigation::{NavAction, TabIndex, TabNavigation};
use bevy::input_focus::{FocusCause, InputFocus, InputFocusVisible, IsFocused, IsFocusedHelper};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use super::theme::{FontAssets, UiTheme};

/// Marker for our themed menu buttons — used by [`style_menu_buttons`] to tint on hover/focus and
/// by [`menu_keyboard_nav`] to detect whether any menu is open.
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
///
/// The button carries [`TabIndex(0)`] so it participates in keyboard navigation ([`menu_keyboard_nav`]).
/// All menu buttons share index `0`, so their visit order is simply their spawn/child order — spawn
/// them top-to-bottom and the keyboard highlight follows suit, with nothing to keep in sync. The
/// screen's button container must carry a [`bevy::input_focus::tab_navigation::TabGroup`] for this
/// to take effect.
pub fn button_visual(theme: &UiTheme) -> impl Bundle {
    (
        Button,
        Hovered::default(),
        MenuButton,
        TabIndex(0),
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

/// Tint for [`MenuButton`]s: highlighted when hovered by the mouse **or** holding keyboard focus.
/// Runs every frame (a menu has only a handful of buttons) because focus changes — unlike
/// [`Hovered`] — don't fire component change-detection; writes are still guarded so unchanged
/// buttons don't churn.
pub fn style_menu_buttons(
    theme: Res<UiTheme>,
    focus: IsFocusedHelper,
    mut buttons: Query<(Entity, &Hovered, &mut BackgroundColor), With<MenuButton>>,
) {
    for (entity, hovered, mut bg) in &mut buttons {
        let active = hovered.0 || focus.is_focus_visible(entity);
        let want = if active {
            theme.panel_border.with_alpha(0.30)
        } else {
            theme.panel
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

/// Keyboard navigation for whichever menu screen is open. Up/Down (or W/S) move the keyboard focus
/// between [`MenuButton`]s using Bevy 0.19's built-in tab navigation ([`TabNavigation`] over the
/// screen's `TabGroup` and per-button [`TabIndex`], which handles wrapping and per-group scoping);
/// Enter/Space then activate the focused button (that part is `bevy_ui_widgets::Button`'s own
/// focused-key handler — this system only decides which entity holds [`InputFocus`]).
///
/// Registered **once**, globally: it early-returns when no menu is open (no [`MenuButton`] exists,
/// hence no `TabGroup`), so every menu screen gets keyboard nav for free with nothing per-screen to
/// wire up or forget. `navigate` only allocates when a nav key is actually pressed (or once on open
/// to seed focus), so idle frames do no work.
pub fn menu_keyboard_nav(
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
    nav: TabNavigation,
    buttons: Query<(), With<MenuButton>>,
) {
    if buttons.is_empty() {
        return; // No menu open — nothing to navigate.
    }

    let up = keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW);
    let down = keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS);
    if !up && !down {
        // Menu just opened with nothing focused yet: focus the first button so Enter is immediately
        // live, but leave the focus ring hidden (`visible` stays false) until the player actually
        // navigates by keyboard — a mouse-only user must never see a stray keyboard highlight.
        if focus.get().is_none() {
            if let Ok(first) = nav.navigate(&focus, NavAction::First) {
                focus.set(first, FocusCause::Navigated);
            }
        }
        return;
    }

    let action = if down { NavAction::Next } else { NavAction::Previous };
    if let Ok(next) = nav.navigate(&focus, action) {
        focus.set(next, FocusCause::Navigated);
        visible.0 = true;
    }
}

/// Mouse hover drives the selection: when the cursor moves onto a [`MenuButton`], make it the focused
/// entity — so Enter/Space (and NumpadEnter, see [`menu_activate_numpad_enter`]) act on the button
/// under the cursor — and hide the keyboard focus ring. This keeps the hover tint and the keyboard
/// ring from lighting two different buttons at once (the ring only ever shows while navigating by
/// keyboard). Runs on `Changed<Hovered>`, so it costs nothing until the cursor crosses a button.
pub fn focus_hovered_menu_button(
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
    hovered: Query<(Entity, &Hovered), (With<MenuButton>, Changed<Hovered>)>,
) {
    for (entity, hov) in &hovered {
        if hov.0 {
            if focus.get() != Some(entity) {
                focus.set(entity, FocusCause::Navigated);
            }
            if visible.0 {
                visible.0 = false;
            }
        }
    }
}

/// Restore numeric-keypad Enter as a menu activation key. `bevy_ui_widgets::Button` only activates on
/// the main-row Enter or Space, so without this NumpadEnter would be silently dead on every menu.
/// Fires the same [`Activate`] event `Button` would, targeted at the focused menu button.
pub fn menu_activate_numpad_enter(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    buttons: Query<(), With<MenuButton>>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::NumpadEnter) {
        return;
    }
    if let Some(entity) = focus.get() {
        if buttons.contains(entity) {
            commands.trigger(Activate { entity });
        }
    }
}

/// Drop keyboard focus whenever no menu is open, so a stale [`InputFocus`] never dangles at a
/// despawned button and the next menu always opens fresh. Global — there is no per-screen `OnExit`
/// to remember to register (which is what let the Settings screen be missed before).
pub fn clear_menu_focus_when_empty(
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
    buttons: Query<(), With<MenuButton>>,
) {
    if buttons.is_empty() {
        if focus.get().is_some() {
            focus.clear();
        }
        if visible.0 {
            visible.0 = false;
        }
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

#[cfg(test)]
mod tests {
    //! GPU-free ECS tests for menu keyboard/mouse focus. These exercise the real systems against a
    //! minimal `App` (no window, no render), so they run under plain `cargo test`. We insert the
    //! focus resources directly rather than adding `InputFocusPlugin`, whose `set_initial_focus`
    //! panics without a `PrimaryWindow`.

    use super::*;
    use bevy::input_focus::tab_navigation::TabGroup;

    /// A menu screen: a `TabGroup` container with `n` [`MenuButton`] children (spawn order = nav
    /// order). Returns the app and the button entities in spawn order.
    fn menu_app(n: usize) -> (App, Vec<Entity>) {
        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .init_resource::<InputFocusVisible>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, menu_keyboard_nav);

        let group = app.world_mut().spawn(TabGroup::new(0)).id();
        let buttons: Vec<Entity> = (0..n)
            .map(|_| {
                app.world_mut()
                    .spawn((MenuButton, TabIndex(0), ChildOf(group)))
                    .id()
            })
            .collect();
        (app, buttons)
    }

    /// Simulate a single fresh key press. `reset_all` first releases any still-held key, so `press`
    /// registers a new `just_pressed` (it won't for a key already in `pressed`) — the real input
    /// plugin clears state each frame; here we do it by hand.
    fn tap(app: &mut App, key: KeyCode) {
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.reset_all();
        input.press(key);
    }

    fn focused(app: &App) -> Option<Entity> {
        app.world().resource::<InputFocus>().get()
    }

    fn ring_visible(app: &App) -> bool {
        app.world().resource::<InputFocusVisible>().0
    }

    #[test]
    fn open_seeds_first_button_with_ring_hidden() {
        // On open (no nav key yet) the first button is focused so Enter is live, but the keyboard
        // ring stays hidden — a mouse-only player must never see a stray highlight (finding #1).
        let (mut app, b) = menu_app(3);
        app.update();
        assert_eq!(focused(&app), Some(b[0]), "first button seeded on open");
        assert!(!ring_visible(&app), "focus ring hidden until the player navigates");
    }

    #[test]
    fn down_and_up_wrap_and_show_ring() {
        let (mut app, b) = menu_app(3);
        app.update(); // seed on b[0]

        tap(&mut app, KeyCode::ArrowDown);
        app.update();
        assert_eq!(focused(&app), Some(b[1]));
        assert!(ring_visible(&app), "navigating shows the keyboard ring");

        tap(&mut app, KeyCode::KeyS); // S == Down
        app.update();
        assert_eq!(focused(&app), Some(b[2]));

        tap(&mut app, KeyCode::ArrowDown);
        app.update();
        assert_eq!(focused(&app), Some(b[0]), "Down wraps past the last item");

        tap(&mut app, KeyCode::ArrowUp);
        app.update();
        assert_eq!(focused(&app), Some(b[2]), "Up wraps past the first item");
    }

    #[test]
    fn no_menu_open_is_inert() {
        // With no MenuButtons, the global system must not touch focus (it runs every frame in-game).
        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .init_resource::<InputFocusVisible>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, menu_keyboard_nav);
        tap(&mut app, KeyCode::ArrowDown);
        app.update();
        assert_eq!(focused(&app), None);
        assert!(!ring_visible(&app));
    }

    #[test]
    fn hover_follows_focus_and_hides_ring() {
        // Hovering a button makes it the focused entity (so Enter acts on the button under the
        // cursor) and hides the keyboard ring, so hover-tint and ring never light two at once (#1).
        let (mut app, b) = menu_app(3);
        app.add_systems(Update, focus_hovered_menu_button);
        app.update(); // seed on b[0], ring hidden

        // Keyboard to b[1] with the ring shown…
        tap(&mut app, KeyCode::ArrowDown);
        app.update();
        assert_eq!(focused(&app), Some(b[1]));
        assert!(ring_visible(&app));

        // …then the cursor lands on b[2]: focus follows the cursor, ring hides.
        {
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            input.clear();
        }
        app.world_mut().entity_mut(b[2]).insert(Hovered(true));
        app.update();
        assert_eq!(focused(&app), Some(b[2]), "focus follows the hovered button");
        assert!(!ring_visible(&app), "mouse hover hides the keyboard ring");
    }
}
