//! Settings menu — reachable from the title (`TitleMenu::Settings`) and the pause menu
//! (`MenuState::Settings`), sharing one panel builder.
//!
//! Grouped **ACCESS** vs **CHALLENGE** after Power, Cairns, Barlet & Haynes 2019, *Future design of
//! accessibility in games: A design vocabulary* (DOI 10.1016/j.ijhcs.2019.06.010), whose brief is
//! *"Games are meant to be difficult but not difficult to access."* Access options describe how the
//! game is presented and operated and never change difficulty; Challenge options describe how much
//! the HUD tells you. See `docs/ui.md` §4 for why the line also decides what the RL/QD search may
//! evolve.
//!
//! **Controls** and **Audio** are shown disabled with a "pending" note until their gated phases
//! (keybind remap / audio overhaul) land.

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
#[derive(Component, Clone, Copy, Debug)]
enum SettingKey {
    // ACCESS — how the game is perceived and operated. Never gated by difficulty.
    TextScale,
    HudScale,
    ReduceFlashing,
    // CHALLENGE — how much the game tells you. Legitimately a difficulty knob.
    BossBar,
    RosterDetail,
}

/// One step of a scale knob, and its bounds. Held here rather than at each call site so the two
/// scales cannot drift apart from `theme::apply_text_scale` / `theme::apply_hud_scale`, which clamp
/// to the same range.
const SCALE_MIN: f32 = 0.75;
const SCALE_MAX: f32 = 1.5;
const SCALE_STEP: f32 = 0.25;

/// Which group a setting belongs to.
///
/// Access options describe the **player** (how the game is presented and operated); Challenge
/// options describe the **game** (how much it tells you). Power et al. 2019 keep them apart so that
/// nobody has to trade legibility for difficulty. In this project the split carries a second load:
/// Challenge options are legitimate RL/QD genome genes, Access options never are — evolving them
/// would be optimising against whoever is sitting at the keyboard.
fn is_access(key: SettingKey) -> bool {
    match key {
        SettingKey::TextScale | SettingKey::HudScale | SettingKey::ReduceFlashing => true,
        SettingKey::BossBar | SettingKey::RosterDetail => false,
    }
}

/// Advance a scale knob, wrapping at the top so one control can walk the whole range.
///
/// Pure and tested: a stepper that silently pinned at the maximum would leave the player unable to
/// get back to 1.0 without editing the settings file.
fn next_scale(current: f32) -> f32 {
    let next = current + SCALE_STEP;
    if next > SCALE_MAX + 1.0e-4 {
        SCALE_MIN
    } else {
        next.clamp(SCALE_MIN, SCALE_MAX)
    }
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
///
/// [`Action::MenuBack`](crate::input::Action::MenuBack) rather than `PauseMenu`, even though both
/// default to `Escape`: they sit in different [`Context`](crate::input::Context)s, which is what
/// lets one key legally serve both and what the collision test checks.
fn settings_escape_to_title(
    actions: crate::input::Actions,
    mut next: ResMut<NextState<TitleMenu>>,
) {
    if actions.just_pressed(crate::input::Action::MenuBack) {
        next.set(TitleMenu::Root);
    }
}

/// Esc from the pause-opened Settings returns to the pause menu (mirrors the "BACK" button).
/// `pause::toggle_pause` also sees this Esc but ignores it while `MenuState::Settings`, so there is
/// no double handling.
fn settings_escape_to_pause(
    actions: crate::input::Actions,
    mut next: ResMut<NextState<MenuState>>,
) {
    if actions.just_pressed(crate::input::Action::MenuBack) {
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

            // The split is Power, Cairns, Barlet & Haynes 2019 (DOI 10.1016/j.ijhcs.2019.06.010),
            // whose design vocabulary separates **Access Options** (Input, Control, Presentation,
            // Output) from **Challenge Options** (Performance, Training, Progress, ...). Their line
            // is the brief: *"Games are meant to be difficult but not difficult to access."*
            //
            // Keeping them apart is not cosmetic. It states, in the UI itself, that turning text up
            // is not turning the game down — so nobody has to trade legibility for difficulty. It
            // also draws the line this project needs elsewhere: Challenge options are legitimate
            // RL/QD genome genes, Access options never are (`docs/ui.md` §4).

            // --- ACCESS ---
            root.spawn(text_colored(theme, fonts, "ACCESS", theme.font_body, theme.accent));
            root.spawn(text_colored(
                theme,
                fonts,
                "How the game is presented. Never changes difficulty.",
                theme.font_body * 0.85,
                theme.text_muted,
            ));
            toggle_button(root, theme, fonts, SettingKey::TextScale);
            toggle_button(root, theme, fonts, SettingKey::HudScale);
            toggle_button(root, theme, fonts, SettingKey::ReduceFlashing);

            // --- CHALLENGE ---
            root.spawn(text_colored(theme, fonts, "CHALLENGE", theme.font_body, theme.accent));
            root.spawn(text_colored(
                theme,
                fonts,
                "How much the HUD tells you.",
                theme.font_body * 0.85,
                theme.text_muted,
            ));
            toggle_button(root, theme, fonts, SettingKey::BossBar);
            toggle_button(root, theme, fonts, SettingKey::RosterDetail);

            // --- Disabled groups (pending gated phases) ---
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
        SettingKey::TextScale => {
            btn.observe(|_: On<Activate>, mut acc: ResMut<AccessibilitySettings>| {
                acc.text_scale = next_scale(acc.text_scale);
            });
        }
        SettingKey::HudScale => {
            btn.observe(|_: On<Activate>, mut hud: ResMut<HudSettings>| {
                hud.hud_scale = next_scale(hud.hud_scale);
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
            SettingKey::TextScale => format!("Text scale:  {:.0}%", acc.text_scale * 100.0),
            SettingKey::HudScale => format!("HUD scale:  {:.0}%", hud.hud_scale * 100.0),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_stepper_walks_the_whole_range_and_wraps() {
        // A stepper that pinned at the maximum would leave a player who overshot unable to get back
        // to 100% without hand-editing the settings file — the setting would be a trap, not a knob.
        let mut v = SCALE_MIN;
        let mut seen = vec![v];
        for _ in 0..16 {
            v = next_scale(v);
            seen.push(v);
            if (v - SCALE_MIN).abs() < 1.0e-4 && seen.len() > 1 {
                break;
            }
        }
        assert!(
            seen.iter().any(|x| (x - 1.0).abs() < 1.0e-4),
            "100% must be reachable: {seen:?}"
        );
        assert!(
            seen.iter().any(|x| (x - SCALE_MAX).abs() < 1.0e-4),
            "the maximum must be reachable: {seen:?}"
        );
        assert_eq!(*seen.last().expect("stepped"), SCALE_MIN, "and it must wrap: {seen:?}");
    }

    #[test]
    fn the_stepper_never_escapes_the_range_the_appliers_clamp_to() {
        // `theme::apply_text_scale` / `apply_hud_scale` clamp to the same bounds. If the stepper
        // could produce a value outside them, the label would report a scale the game never applied.
        let mut v = SCALE_MIN;
        for _ in 0..50 {
            v = next_scale(v);
            assert!(
                (SCALE_MIN..=SCALE_MAX).contains(&v),
                "stepper produced {v}, outside {SCALE_MIN}..={SCALE_MAX}"
            );
        }
    }

    #[test]
    fn access_and_challenge_are_not_mixed_up() {
        // The split is the point (Power et al. 2019): turning text up must never read as turning the
        // game down. Pins which group each key belongs to, so a new setting cannot be filed by
        // accident into the group that carries the opposite promise.
        for k in [SettingKey::TextScale, SettingKey::HudScale, SettingKey::ReduceFlashing] {
            assert!(is_access(k), "{k:?} is an Access option");
        }
        for k in [SettingKey::BossBar, SettingKey::RosterDetail] {
            assert!(!is_access(k), "{k:?} is a Challenge option");
        }
    }
}
