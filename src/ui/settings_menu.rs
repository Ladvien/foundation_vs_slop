//! Settings menu — reachable from the title (`TitleMenu::Settings`) and the pause menu
//! (`MenuState::Settings`), sharing one panel builder. Only the **Display** and **Accessibility**
//! groups are live; **Controls** and **Audio** are shown disabled with a "pending" note until their
//! gated phases (keybind remap / audio overhaul) land.

use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::settings::{AccessibilitySettings, HudSettings, RosterDetail};

use super::state::{MenuState, TitleMenu};
use super::theme::{FontAssets, UiTheme, Z_MENU};
use super::widgets::{button_visual, text, text_colored};

/// Root marker for the settings panel (despawned on exit of either owning state).
#[derive(Component)]
pub struct SettingsRoot;

/// Which live setting a toggle button's label reflects (kept in sync by [`refresh_setting_labels`]).
#[derive(Component, Clone, Copy)]
enum SettingKey {
    BossBar,
    RosterDetail,
    Colorblind,
    ReduceFlashing,
}

/// Where "Back" returns to, depending on where Settings was opened from.
#[derive(Clone, Copy)]
enum BackTo {
    Title,
    Pause,
}

pub struct SettingsMenuPlugin;

impl Plugin for SettingsMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(TitleMenu::Settings), spawn_from_title)
            .add_systems(
                OnExit(TitleMenu::Settings),
                super::state::despawn_scoped::<SettingsRoot>,
            )
            .add_systems(OnEnter(MenuState::Settings), spawn_from_pause)
            .add_systems(
                OnExit(MenuState::Settings),
                super::state::despawn_scoped::<SettingsRoot>,
            )
            .add_systems(
                Update,
                refresh_setting_labels
                    .run_if(in_state(TitleMenu::Settings).or_else(in_state(MenuState::Settings))),
            )
            // Esc backs out to wherever Settings was opened from, so a keyboard-only player is never
            // trapped here (there is no camera Esc handler while a blocking screen is up). Which
            // system runs is decided by the active state, matching the "BACK" button's target.
            .add_systems(
                Update,
                settings_escape_to_title.run_if(in_state(TitleMenu::Settings)),
            )
            .add_systems(
                Update,
                settings_escape_to_pause.run_if(in_state(MenuState::Settings)),
            );
    }
}

/// Esc from the title-opened Settings returns to the title root (mirrors the "BACK" button).
fn settings_escape_to_title(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<TitleMenu>>) {
    if keys.just_pressed(KeyCode::Escape) {
        next.set(TitleMenu::Root);
    }
}

/// Esc from the pause-opened Settings returns to the pause menu (mirrors the "BACK" button).
/// `pause::toggle_pause` also sees this Esc but ignores it while `MenuState::Settings`, so there is
/// no double handling.
fn settings_escape_to_pause(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<MenuState>>) {
    if keys.just_pressed(KeyCode::Escape) {
        next.set(MenuState::Pause);
    }
}

fn spawn_from_title(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    spawn_settings(&mut commands, &theme, &fonts, BackTo::Title);
}

fn spawn_from_pause(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    spawn_settings(&mut commands, &theme, &fonts, BackTo::Pause);
}

fn spawn_settings(commands: &mut Commands, theme: &UiTheme, fonts: &FontAssets, back: BackTo) {
    commands
        .spawn((
            SettingsRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(theme.space_md),
                ..default()
            },
            BackgroundColor(theme.bg),
            GlobalZIndex(Z_MENU),
            // Scopes keyboard nav to this panel's toggles + Back (their `TabIndex` is inert without it).
            TabGroup::new(0),
        ))
        .with_children(|root| {
            root.spawn(text_colored(theme, fonts, "SETTINGS", theme.font_title * 0.6, theme.accent));

            // --- DISPLAY ---
            root.spawn(text_colored(theme, fonts, "DISPLAY", theme.font_body, theme.text_muted));
            toggle_button(root, theme, fonts, SettingKey::BossBar);
            toggle_button(root, theme, fonts, SettingKey::RosterDetail);

            // --- ACCESSIBILITY ---
            root.spawn(text_colored(
                theme,
                fonts,
                "ACCESSIBILITY",
                theme.font_body,
                theme.text_muted,
            ));
            toggle_button(root, theme, fonts, SettingKey::Colorblind);
            toggle_button(root, theme, fonts, SettingKey::ReduceFlashing);

            // --- Disabled groups (pending gated phases) ---
            root.spawn(text_colored(
                theme,
                fonts,
                "CONTROLS  — pending keybind remap",
                theme.font_body,
                theme.text_muted.with_alpha(0.5),
            ));
            root.spawn(text_colored(
                theme,
                fonts,
                "AUDIO  — pending audio overhaul",
                theme.font_body,
                theme.text_muted.with_alpha(0.5),
            ));

            // --- Back ---
            let mut back_btn = root.spawn(button_visual(theme));
            back_btn.with_children(|b| {
                b.spawn(text(theme, fonts, "BACK", theme.font_body));
            });
            match back {
                BackTo::Title => {
                    back_btn.observe(|_: On<Activate>, mut next: ResMut<NextState<TitleMenu>>| {
                        next.set(TitleMenu::Root);
                    });
                }
                BackTo::Pause => {
                    back_btn.observe(|_: On<Activate>, mut next: ResMut<NextState<MenuState>>| {
                        next.set(MenuState::Pause);
                    });
                }
            }
        });
}

/// Spawn a labelled toggle button that flips its backing setting on click. The label text carries a
/// [`SettingKey`] so [`refresh_setting_labels`] can show the live value.
fn toggle_button(
    parent: &mut bevy::ecs::relationship::RelatedSpawnerCommands<ChildOf>,
    theme: &UiTheme,
    fonts: &FontAssets,
    key: SettingKey,
) {
    let mut btn = parent.spawn(button_visual(theme));
    btn.with_children(|b| {
        b.spawn((text(theme, fonts, "…", theme.font_body), key));
    });
    match key {
        SettingKey::BossBar => {
            btn.observe(|_: On<Activate>, mut hud: ResMut<HudSettings>| {
                hud.show_boss_bar = !hud.show_boss_bar;
            });
        }
        SettingKey::RosterDetail => {
            btn.observe(|_: On<Activate>, mut hud: ResMut<HudSettings>| {
                hud.roster_detail = match hud.roster_detail {
                    RosterDetail::Full => RosterDetail::Compact,
                    RosterDetail::Compact => RosterDetail::Hidden,
                    RosterDetail::Hidden => RosterDetail::Full,
                };
            });
        }
        SettingKey::Colorblind => {
            btn.observe(|_: On<Activate>, mut acc: ResMut<AccessibilitySettings>| {
                acc.colorblind_safe = !acc.colorblind_safe;
            });
        }
        SettingKey::ReduceFlashing => {
            btn.observe(|_: On<Activate>, mut acc: ResMut<AccessibilitySettings>| {
                acc.reduce_flashing = !acc.reduce_flashing;
            });
        }
    }
}

/// Keep each toggle button's label in sync with the live setting value.
fn refresh_setting_labels(
    hud: Res<HudSettings>,
    acc: Res<AccessibilitySettings>,
    mut labels: Query<(&SettingKey, &mut Text)>,
) {
    for (key, mut label) in &mut labels {
        let s = match key {
            SettingKey::BossBar => format!("Boss bar:  {}", on_off(hud.show_boss_bar)),
            SettingKey::RosterDetail => format!("Roster detail:  {}", roster_label(hud.roster_detail)),
            SettingKey::Colorblind => format!("Colorblind-safe palette:  {}", on_off(acc.colorblind_safe)),
            SettingKey::ReduceFlashing => format!("Reduce flashing:  {}", on_off(acc.reduce_flashing)),
        };
        if label.0 != s {
            label.0 = s;
        }
    }
}

fn on_off(b: bool) -> &'static str {
    if b {
        "ON"
    } else {
        "OFF"
    }
}

fn roster_label(d: RosterDetail) -> &'static str {
    match d {
        RosterDetail::Full => "FULL",
        RosterDetail::Compact => "COMPACT",
        RosterDetail::Hidden => "HIDDEN",
    }
}
