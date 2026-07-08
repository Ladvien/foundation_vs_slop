//! In-game HUD (clear overlay). This first slice reads only collision-free sim state:
//! - **Squad roster strip** (bottom-left): one chip per [`Unit`] with its [`Outfit`] color and a
//!   live [`Health`] bar.
//! - **Time/speed readout** (bottom-right): the [`GameSpeed`] rung, or `PAUSED`.
//!
//! Boss bar, minimap, threat arrows, command ping and player-controllable density arrive in later
//! phases (the minimap waits on the concurrent `dungeon.rs` rewrite). Every HUD element is
//! non-diegetic and ignores pointer input so world clicks pass through.

use bevy::prelude::*;

use crate::health::Health;
use crate::squad::{Outfit, Unit};
use crate::time_control::GameSpeed;

use super::state::AppState;
use super::theme::{FontAssets, UiTheme, Z_HUD};
use super::widgets::{bar_back, bar_fill, border_all, text_colored};

/// Root marker for the whole HUD (despawned on leaving `InGame`).
#[derive(Component)]
pub struct HudRoot;

/// A health-bar fill node bound to the unit whose health it shows.
#[derive(Component)]
pub struct HealthFillOf {
    pub unit: Entity,
}

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
                (update_health_fills, update_speed_text).run_if(in_state(AppState::InGame)),
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
            // --- Squad roster strip (bottom-left) ---
            root.spawn((
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
                            // Team-color swatch.
                            chip.spawn((
                                Node {
                                    width: Val::Px(28.0),
                                    height: Val::Px(6.0),
                                    ..default()
                                },
                                BackgroundColor(outfit.0),
                            ));
                            // Health bar (back + bound fill).
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

/// Resize each bound health-fill node to its unit's current health fraction. A despawned unit
/// collapses its bar to zero (defensive — units don't currently despawn, but bosses/enemies do
/// and this pattern will be reused for them).
fn update_health_fills(
    healths: Query<&Health>,
    mut fills: Query<(&HealthFillOf, &mut Node)>,
) {
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
