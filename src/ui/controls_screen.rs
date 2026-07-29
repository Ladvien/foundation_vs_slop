//! **The controls screen** — the list of what the keys do, which this game did not have.
//!
//! # The gap this fills
//!
//! Before `crate::input` existed there was nowhere to *put* such a list: the bindings were ~30 raw
//! `KeyCode` literals scattered across fourteen modules, coordinated by five prose comments that had
//! already drifted. So the game shipped roughly **26 player-facing bindings of which 6 were stated
//! anywhere** — the four verb chips (each of which leads with its key, enforced by a test), plus
//! `[TAB] SELECT SPECIMEN` and `[R] RUN THE TOP TEST` on the Site panels. Left-click, right-click,
//! `WASD`, `Q`/`E`, the wheel, `Space`, `H`, `L`, `K`, `J`, `B`/`N`/`M` and the control groups were
//! discoverable only by being told.
//!
//! The number that makes this urgent: Iacovides, Cox, Kennedy, Cairns & Jennett 2015
//! (DOI 10.1145/2793107.2793120) had to **drop 7 of 31 screened participants** — every one of them a
//! self-reported FPS player — for "obviously struggling with the controls" inside a 20-minute
//! session. ~23% of a genre-experienced sample could not clear a controls barrier. This screen does
//! not fix that on its own, but its absence guarantees it.
//!
//! # Why it lists rather than remaps (yet)
//!
//! `input::KeyBindings::rebind` is written, tested, and refuses a colliding chord while *naming* the
//! action that already owns it — the hard half. What is missing is the press-a-key capture widget.
//! Rather than ship a half-interactive screen, this states every binding and says plainly that
//! remapping is not wired yet, per `docs/ui.md` §1.4: **an unmet condition is an instruction, and an
//! empty panel reads as a bug.** The line names the state, not a padlock.
//!
//! Access-side (Power/Cairns et al. 2019), so it is reachable from the title *and* from the pause
//! menu and is never gated by difficulty.

use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, ScrollArea};

use super::state::{despawn_scoped, MenuState, TitleMenu};
use super::theme::{FontAssets, UiTheme, Z_MENU};
use super::widgets::{button_visual, text, text_colored};
use crate::input::{Action, Context, KeyBindings};

/// Root marker for the screen.
#[derive(Component)]
pub struct ControlsRoot;

/// Where "BACK" returns to — the screen this was opened from.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum BackTo {
    Title,
    Pause,
}

pub struct ControlsScreenPlugin;

impl Plugin for ControlsScreenPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(TitleMenu::Controls), spawn_from_title)
            .add_systems(OnExit(TitleMenu::Controls), despawn_scoped::<ControlsRoot>)
            .add_systems(OnEnter(MenuState::Controls), spawn_from_pause)
            .add_systems(OnExit(MenuState::Controls), despawn_scoped::<ControlsRoot>)
            .add_systems(
                Update,
                escape_to_title.run_if(in_state(TitleMenu::Controls)),
            )
            .add_systems(
                Update,
                escape_to_pause.run_if(in_state(MenuState::Controls)),
            );
    }
}

fn escape_to_title(actions: crate::input::Actions, mut next: ResMut<NextState<TitleMenu>>) {
    if actions.just_pressed(Action::MenuBack) {
        next.set(TitleMenu::Root);
    }
}

fn escape_to_pause(actions: crate::input::Actions, mut next: ResMut<NextState<MenuState>>) {
    if actions.just_pressed(Action::MenuBack) {
        next.set(MenuState::Pause);
    }
}

fn spawn_from_title(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    bindings: Res<KeyBindings>,
) {
    spawn(&mut commands, &theme, &fonts, &bindings, BackTo::Title);
}

fn spawn_from_pause(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    bindings: Res<KeyBindings>,
) {
    spawn(&mut commands, &theme, &fonts, &bindings, BackTo::Pause);
}

/// One printable row: the instruction and the chord that performs it.
///
/// Pure and separate from the spawn so the *content* of the screen is unit-testable without a UI
/// tree — the same discipline `ui::rows` sets for every other panel.
pub fn control_lines(bindings: &KeyBindings) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    // Grouped by context, in the order a player meets them. Dev actions are omitted entirely: they
    // do not exist in a release build, and listing keys that do nothing is worse than listing none.
    for group in [Context::Play, Context::InGame, Context::Site, Context::Menu] {
        for a in Action::ALL {
            if a.context() != group || a.is_dev() {
                continue;
            }
            out.push((a.label(), bindings.get(a).label()));
        }
    }
    out
}

/// The mouse and the control-group row, which are real bindings with no [`Action`] behind them —
/// the mouse because buttons are not keys, the digits because nine numbered slots are one mechanism
/// rather than nine rebindable actions (see `selection::control_group_input`).
///
/// Listing them here is the point: a controls screen that only showed what happened to be in the
/// registry would omit *the two most important inputs in the game*.
pub const UNBOUND_LINES: &[(&str, &str)] = &[
    ("SELECT A UNIT", "Left click"),
    ("SELECT SEVERAL", "Left drag"),
    ("ADD TO SELECTION", "Shift + left click"),
    ("SELECT THAT ROLE", "Double left click"),
    ("SELECT THE SQUAD", "Ctrl+A"),
    ("MOVE ORDER", "Right click"),
    ("QUEUE A MOVE ORDER", "Shift + right click"),
    ("PUT THE VERB AWAY", "Right click, while armed"),
    ("RECALL A CONTROL GROUP", "1 - 9"),
    ("BIND A CONTROL GROUP", "Ctrl + 1 - 9"),
    ("ZOOM", "Mouse wheel"),
    ("DRAG THE VIEW", "Middle mouse"),
];

fn spawn(
    commands: &mut Commands,
    theme: &UiTheme,
    fonts: &FontAssets,
    bindings: &KeyBindings,
    back: BackTo,
) {
    commands
        .spawn((
            ControlsRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(theme.space_sm),
                ..default()
            },
            BackgroundColor(theme.bg),
            GlobalZIndex(Z_MENU),
            TabGroup::new(0),
        ))
        .with_children(|root| {
            root.spawn(text_colored(theme, fonts, "CONTROLS", theme.font_title * 0.6, theme.accent));

            // The list is longer than any screen, so it scrolls — `ScrollArea` + `Overflow::scroll_y`,
            // the same pair `site_hud` and `research_hud` already use.
            root.spawn((
                ScrollArea::default(),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(theme.space_xs),
                    max_height: Val::Percent(62.0),
                    min_width: Val::Px(460.0),
                    padding: UiRect::all(Val::Px(theme.space_md)),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|list| {
                for (label, chord) in &UNBOUND_LINES[..] {
                    row(list, theme, fonts, label, chord);
                }
                for (label, chord) in control_lines(bindings) {
                    row(list, theme, fonts, label, &chord);
                }
            });

            // `docs/ui.md` §1.4: state what this is, do not show a padlock and do not go blank.
            root.spawn(text_colored(
                theme,
                fonts,
                "REMAPPING IS NOT WIRED YET — THESE ARE THE SHIPPED KEYS",
                theme.font_body * 0.85,
                theme.text_muted,
            ));

            // `button_visual` already carries `MenuButton` + `TabIndex` (that is what makes the
            // global keyboard nav in `ui::widgets` find it), so only the destination is added here.
            // Adding them again is a *duplicate-component panic* in Bevy 0.19, not a no-op.
            let mut back_btn = root.spawn((button_visual(theme), back));
            back_btn.with_children(|b| {
                b.spawn(text(theme, fonts, "BACK", theme.font_body));
            });
            back_btn.observe(
                |activate: On<Activate>,
                 backs: Query<&BackTo>,
                 mut title: ResMut<NextState<TitleMenu>>,
                 mut menu: ResMut<NextState<MenuState>>| {
                    match backs.get(activate.entity).copied() {
                        Ok(BackTo::Title) => title.set(TitleMenu::Root),
                        Ok(BackTo::Pause) => menu.set(MenuState::Pause),
                        Err(_) => {}
                    }
                },
            );
        });
}

/// One instruction/chord pair on a shared column rhythm, so the chords line up as a scannable
/// column rather than trailing each label at a different x (`ui::rows`' argument, in miniature).
fn row(parent: &mut ChildSpawnerCommands, theme: &UiTheme, fonts: &FontAssets, label: &str, chord: &str) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                column_gap: Val::Px(theme.space_lg),
                width: Val::Percent(100.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|r| {
            r.spawn(text_colored(theme, fonts, label, theme.font_body, theme.text));
            r.spawn(text_colored(theme, fonts, chord, theme.font_body, theme.accent));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_player_binding_is_listed() {
        // The whole reason this screen exists. If an action is added and this list does not grow,
        // the game has gained an undiscoverable key — which is the state it was in before.
        let bindings = KeyBindings::default();
        let lines = control_lines(&bindings);
        let expected = Action::ALL.iter().filter(|a| !a.is_dev()).count();
        assert_eq!(lines.len(), expected, "every non-dev action must appear exactly once");
    }

    #[test]
    fn no_dev_key_is_shown_to_a_player() {
        // Dev actions are compiled out of a release build, so listing them would be listing keys
        // that do nothing.
        let lines = control_lines(&KeyBindings::default());
        for dev in Action::ALL.iter().filter(|a| a.is_dev()) {
            assert!(
                !lines.iter().any(|(l, _)| *l == dev.label()),
                "{dev:?} is a dev key and must not be listed"
            );
        }
    }

    #[test]
    fn no_line_is_blank_on_either_side() {
        // A row with an empty chord reads as "this does nothing"; a row with an empty label reads as
        // a rendering bug. `docs/ui.md` §1.4 — say the state, never show a gap.
        for (label, chord) in control_lines(&KeyBindings::default()) {
            assert!(!label.trim().is_empty(), "an action reached the screen with no name");
            assert!(!chord.trim().is_empty(), "{label} is listed with no key");
            assert_ne!(chord.trim(), "—", "{label} has no resolvable chord to print");
        }
        for (label, chord) in UNBOUND_LINES {
            assert!(!label.trim().is_empty() && !chord.trim().is_empty());
        }
    }

    #[test]
    fn the_mouse_and_the_control_groups_are_listed_even_though_they_are_not_actions() {
        // The two most-used inputs in the game have no `Action` behind them — buttons are not keys,
        // and the digit row is one mechanism rather than nine bindings. A screen generated purely
        // from the registry would silently omit both, which would be the original bug with extra
        // steps.
        let joined: Vec<&str> = UNBOUND_LINES.iter().map(|(l, _)| *l).collect();
        assert!(joined.iter().any(|l| l.contains("MOVE ORDER")));
        assert!(joined.iter().any(|l| l.contains("CONTROL GROUP")));
        assert!(joined.iter().any(|l| l.contains("SELECT A UNIT")));
    }
}
