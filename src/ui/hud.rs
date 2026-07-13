//! In-game HUD (clear overlay). Reads collision-free sim state only:
//! - **Squad roster strip** (bottom-left): one chip per [`Unit`] with [`Outfit`] color + live health.
//! - **Boss bar** (top-center): appears once the Smiley boss is engaged; shows HP + calm/angry.
//! - **Time/speed readout** (bottom-right): the [`GameSpeed`] rung, or `PAUSED`.
//!
//! **Player-controllable density** ([Game-UI Guidance §2]): [`HudSettings`] toggles the roster
//! detail and boss bar; the `H` key cycles a density preset. `hud_scale` live-apply and the minimap
//! come with later phases. Every HUD element is non-diegetic and ignores pointer input.

use bevy::prelude::*;

use crate::enemy::{Enemy, SmileyState};
use crate::health::Health;
use crate::settings::{HudSettings, RosterDetail};
use crate::squad::{Outfit, Unit};
use crate::time_control::GameSpeed;

use super::state::AppState;
use super::theme::{FontAssets, UiTheme, Z_HUD};
use super::widgets::{bar_back, bar_fill, border_all, text_colored};

/// Root marker for the whole HUD (despawned on leaving `InGame`).
#[derive(Component)]
pub struct HudRoot;

/// The roster strip container (toggled by roster-detail density).
#[derive(Component)]
pub struct RosterStripRoot;

/// The boss-bar container (shown only while the boss is engaged + `show_boss_bar`).
#[derive(Component)]
pub struct BossBarRoot;

/// A health-bar fill node bound to the unit whose health it shows.
#[derive(Component)]
pub struct HealthFillOf {
    pub unit: Entity,
}

/// The boss HP fill node.
#[derive(Component)]
pub struct BossHpFill;

/// The boss state label ("WATCHING" / "UNLEASHING").
#[derive(Component)]
pub struct BossStateText;

/// The time/speed readout text node.
#[derive(Component)]
pub struct SpeedText;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::InGame), spawn_hud)
            .add_systems(
                OnExit(AppState::InGame),
                super::state::despawn_scoped::<HudRoot>,
            )
            .add_systems(
                Update,
                (
                    update_health_fills,
                    update_speed_text,
                    update_boss_bar,
                    cycle_density_key,
                    apply_hud_settings,
                )
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

fn spawn_hud(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    units: Query<(Entity, &Outfit), With<Unit>>,
) {
    commands
        .spawn((
            HudRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            GlobalZIndex(Z_HUD),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            // --- Boss bar (top-center), hidden until engaged ---
            root.spawn((
                BossBarRoot,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(theme.space_md),
                    width: Val::Percent(100.0),
                    display: Display::None,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|bar| {
                bar.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(theme.space_xs),
                        padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_sm)),
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    border_all(theme.panel_border),
                    Pickable::IGNORE,
                ))
                .with_children(|panel| {
                    panel.spawn((
                        text_colored(&theme, &fonts, "THE WATCHER", theme.font_body, theme.danger),
                        BossStateText,
                    ));
                    panel
                        .spawn(bar_back(&theme, 360.0, 10.0))
                        .with_children(|back| {
                            back.spawn((bar_fill(1.0, theme.danger), BossHpFill));
                        });
                });
            });

            // --- Squad roster strip (bottom-left) ---
            root.spawn((
                RosterStripRoot,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(theme.space_md),
                    bottom: Val::Px(theme.space_md),
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(theme.space_sm),
                    padding: UiRect::all(Val::Px(theme.space_sm)),
                    ..default()
                },
                BackgroundColor(theme.panel),
                border_all(theme.panel_border),
                Pickable::IGNORE,
            ))
            .with_children(|strip| {
                for (unit, outfit) in &units {
                    strip
                        .spawn((
                            Node {
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                row_gap: Val::Px(theme.space_xs),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|chip| {
                            chip.spawn((
                                Node {
                                    width: Val::Px(28.0),
                                    height: Val::Px(6.0),
                                    ..default()
                                },
                                BackgroundColor(outfit.0),
                            ));
                            chip.spawn(bar_back(&theme, 28.0, 7.0)).with_children(|back| {
                                back.spawn((bar_fill(1.0, theme.health_fill), HealthFillOf { unit }));
                            });
                        });
                }
            });

            // --- Time / speed readout (bottom-right) ---
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(theme.space_md),
                    bottom: Val::Px(theme.space_md),
                    padding: UiRect::axes(Val::Px(theme.space_sm), Val::Px(theme.space_xs)),
                    ..default()
                },
                BackgroundColor(theme.panel),
                border_all(theme.panel_border),
                Pickable::IGNORE,
            ))
            .with_children(|readout| {
                readout.spawn((
                    text_colored(&theme, &fonts, "x1.0", theme.font_body, theme.accent),
                    SpeedText,
                ));
            });
        });
}

/// Resize each bound health-fill node to its unit's current health fraction.
fn update_health_fills(healths: Query<&Health>, mut fills: Query<(&HealthFillOf, &mut Node)>) {
    for (bound, mut node) in &mut fills {
        let frac = healths.get(bound.unit).map(Health::fraction).unwrap_or(0.0);
        node.width = Val::Percent(frac.clamp(0.0, 1.0) * 100.0);
    }
}

/// Mirror the current game speed / pause state into the readout text.
fn update_speed_text(speed: Res<GameSpeed>, mut text_q: Query<&mut Text, With<SpeedText>>) {
    let Ok(mut t) = text_q.single_mut() else {
        return;
    };
    let label = if speed.paused {
        "PAUSED".to_string()
    } else {
        format!("x{:.2}", speed.base)
    };
    if t.0 != label {
        t.0 = label;
    }
}

/// Show the boss bar once the Smiley boss is engaged (has taken damage or turned angry) and the
/// player hasn't hidden it; update its HP fill + hazard-tier label/color. Read-only of `enemy.rs`.
///
/// The bar fill and label follow the **SCP ACS Risk hazard ramp** (green → amber → red), driven by the
/// watcher's mood: `Scared` (fleeing, playing harmless — de-escalated) reads green (Notice), `Watching`
/// (the concealed calm, staring) reads amber (Caution), and `Unleashing` (mask off, instant-kill
/// lightning) reads red (Critical). One glance at the bar's color says how close the mask is to coming
/// off.
fn update_boss_bar(
    hud: Res<HudSettings>,
    theme: Res<UiTheme>,
    boss: Query<(&Health, &SmileyState), With<Enemy>>,
    mut root: Query<&mut Node, With<BossBarRoot>>,
    mut fill: Query<(&mut Node, &mut BackgroundColor), (With<BossHpFill>, Without<BossBarRoot>)>,
    mut label: Query<(&mut Text, &mut TextColor), With<BossStateText>>,
) {
    let Ok(mut root_node) = root.single_mut() else {
        return;
    };

    let engaged = boss.iter().find_map(|(health, state)| {
        let hit = health.current < health.max;
        (hit || state.is_angry()).then(|| {
            // ACS hazard ramp: mood → (Risk color, readout).
            let (color, text): (Color, &str) = if state.is_angry() {
                (theme.danger, "THE WATCHER — UNLEASHING") // red — Critical
            } else if state.is_watching() {
                (theme.warn, "THE WATCHER — WATCHING") // amber — Caution
            } else {
                (theme.health_fill, "THE WATCHER — RECOILING") // green — Notice (Scared/fleeing)
            };
            (health.fraction(), color, text)
        })
    });

    match engaged {
        Some((frac, color, text)) if hud.show_boss_bar => {
            root_node.display = Display::Flex;
            if let Ok((mut f, mut bg)) = fill.single_mut() {
                f.width = Val::Percent(frac.clamp(0.0, 1.0) * 100.0);
                if bg.0 != color {
                    bg.0 = color;
                }
            }
            if let Ok((mut t, mut tc)) = label.single_mut() {
                if t.0 != text {
                    t.0 = text.to_string();
                }
                if tc.0 != color {
                    tc.0 = color;
                }
            }
        }
        _ => root_node.display = Display::None,
    }
}

/// `H` cycles the roster-detail density preset (Full → Compact → Hidden → …). The §2 backbone made
/// operable at the keyboard; the same values are exposed in the settings menu and persisted.
fn cycle_density_key(keys: Res<ButtonInput<KeyCode>>, mut hud: ResMut<HudSettings>) {
    if keys.just_pressed(KeyCode::KeyH) {
        hud.roster_detail = match hud.roster_detail {
            RosterDetail::Full => RosterDetail::Compact,
            RosterDetail::Compact => RosterDetail::Hidden,
            RosterDetail::Hidden => RosterDetail::Full,
        };
    }
}

/// Apply HUD-density settings to node visibility (runs only when settings change). Compact vs Full
/// currently differ only in intent; the visual distinction (hide swatches) lands with `hud_scale`.
fn apply_hud_settings(hud: Res<HudSettings>, mut roster: Query<&mut Node, With<RosterStripRoot>>) {
    if !hud.is_changed() {
        return;
    }
    if let Ok(mut node) = roster.single_mut() {
        node.display = match hud.roster_detail {
            RosterDetail::Hidden => Display::None,
            _ => Display::Flex,
        };
    }
}
