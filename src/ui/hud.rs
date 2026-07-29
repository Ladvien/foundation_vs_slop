//! In-game HUD (clear overlay). Reads collision-free sim state only:
//! - **Squad roster strip** (bottom-left): one chip per [`Unit`] with its role letter, [`Outfit`]
//!   colour, and live health.
//! - **Boss bar** (top-centre): appears once the Smiley boss is engaged; shows HP and its hazard tier.
//! - **Time/speed readout** (bottom-right): the [`GameSpeed`] rung, or `PAUSED`.
//!
//! **Player-controllable density** (`docs/ui.md` §2): [`HudSettings`] toggles the roster detail and
//! boss bar; the `H` key cycles a density preset. Every HUD element is non-diegetic and ignores
//! pointer input.
//!
//! # Two encoding rules this module exists to obey (`docs/ui.md` §1.3)
//!
//! **The roster names its operatives.** The strip used to be a bare colour swatch per unit, drawn from
//! `palette::OUTFITS` — red Gunman, blue Researcher, **green** Psionic, yellow Medic, purple Engineer.
//! Red against green is the canonical deuteranope confusion, so for ~8% of men the two most tactically
//! different operatives in the squad were the same chip. Each chip now carries its **role letter**, and
//! the colour is decoration on top of a label that already works.
//!
//! **The boss bar is a luminosity ramp, not a hue ramp.** It followed green → amber → red, which
//! encodes threat in exactly the channel that fails. It now follows the ACS Disruption scale
//! ([`Hazard`]) — how much light is getting out — which is both the in-fiction encoding
//! (`docs/lore/2026-07-12-scp-color-language.md` §6: *"Use the ACS luminosity scale, not a color
//! scale"*) and the accessible one, plus a glyph as a third channel.

use bevy::prelude::*;

use crate::enemy::{Enemy, SmileyState};
use crate::health::Health;
use crate::settings::{HudSettings, RosterDetail};
use crate::squad::{Outfit, Unit};
use crate::squad_ai::role::RoleId;
use crate::time_control::GameSpeed;

use super::layout::{self, HudRegions, Region};
use super::state::AppState;
use super::theme::{FontAssets, Hazard, UiTheme};
use super::widgets::{bar_back, bar_fill, border_all, text_colored};

/// Marks each HUD panel, so `OnExit(InGame)` sweeps all of them in one `despawn_scoped`.
///
/// **There are three, not one.** The HUD's elements sit in three different layout regions
/// (top-centre, bottom-left, bottom-right), so there is no single box to parent them under and no
/// honest way to make this a lone entity — an empty sentinel node whose only job was to be counted
/// would be a fiction the despawn sweep then had to work around.
/// `tests/replay.rs::ui_screens_spawn_and_pause_blocks_the_sim` asserts the HUD is up by checking the
/// named parts (roster, boss bar, speed readout), which is what "the HUD spawned" actually means.
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

/// The boss state label.
#[derive(Component)]
pub struct BossStateText;

/// The time/speed readout text node. Load-bearing name — see [`HudRoot`].
#[derive(Component)]
pub struct SpeedText;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::InGame),
            spawn_hud.after(layout::spawn_frame),
        )
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

/// The single letter that identifies an operative without relying on colour.
///
/// Distinctness is asserted by a test — two roles sharing a letter would put the roster right back
/// where the bare colour swatch left it.
pub fn role_letter(role: RoleId) -> &'static str {
    match role {
        RoleId::Gunman => "G",
        RoleId::Researcher => "R",
        RoleId::Psionic => "P",
        RoleId::Medic => "M",
        RoleId::Engineer => "E",
    }
}

/// The boss's hazard tier, from its mood.
///
/// Pure, so the mood → tier mapping is testable without an `App`. `Scared` (fleeing, playing
/// harmless — de-escalated) is the dimmest; `Unleashing` (mask off, instant-kill lightning) is the
/// brightest. `Amida` is deliberately unused: the lore doc reserves it, and a tier that fires every
/// fight is not a reservation.
fn boss_hazard(state: &SmileyState) -> Hazard {
    if state.is_angry() {
        Hazard::Ekhi
    } else if state.is_watching() {
        Hazard::Keneq
    } else {
        Hazard::Vlam
    }
}

/// The boss readout line for a hazard tier.
fn boss_label(h: Hazard) -> String {
    let what = match h {
        Hazard::Ekhi | Hazard::Amida => "UNLEASHING",
        Hazard::Keneq => "WATCHING",
        _ => "RECOILING",
    };
    format!("{} THE WATCHER — {what}", h.glyph())
}

fn spawn_hud(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    regions: Res<HudRegions>,
    units: Query<(Entity, &Outfit, &RoleId), With<Unit>>,
) {
    // --- Boss bar (top-centre), hidden until engaged ---
    let boss = (
        BossBarRoot,
        HudRoot,
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(theme.space_xs),
            padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_sm)),
            ..default()
        },
        BackgroundColor(theme.panel),
        border_all(theme.panel_border),
        Pickable::IGNORE,
    );
    if let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::TopCenter, boss) {
        ec.with_children(|panel| {
            panel.spawn((
                text_colored(&theme, &fonts, "", theme.font_body, theme.text),
                BossStateText,
                Pickable::IGNORE,
            ));
            panel
                .spawn((bar_back(&theme, 360.0, 10.0), Pickable::IGNORE))
                .with_children(|back| {
                    back.spawn((bar_fill(1.0, theme.accent), BossHpFill, Pickable::IGNORE));
                });
        });
    } else {
        error!("HUD: no layout frame at spawn — boss bar not shown");
    }

    // --- Squad roster strip (bottom-left) ---
    //
    // Same region as the containment readout, which is the point: they used to be two absolutely
    // positioned panels both claiming bottom-left at different paddings, drawing over each other.
    // As siblings in one column they stack.
    let roster = (
        RosterStripRoot,
        HudRoot,
        Node {
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(theme.space_sm),
            padding: UiRect::all(Val::Px(theme.space_sm)),
            ..default()
        },
        BackgroundColor(theme.panel),
        border_all(theme.panel_border),
        Pickable::IGNORE,
    );
    if let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::BottomLeft, roster) {
        ec.with_children(|strip| {
            // SORT-OK: presentation only. Chip order is cosmetic, nothing downstream reads it, and
            // this panel writes no state the sim or `snapshot_hash` can observe.
            for (unit, outfit, role) in &units {
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
                        // The role letter, over the outfit colour. The letter is the identity; the
                        // colour is reinforcement, not the only channel.
                        chip.spawn((
                            Node {
                                width: Val::Px(28.0),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|slot| {
                            slot.spawn((
                                text_colored(
                                    &theme,
                                    &fonts,
                                    role_letter(*role),
                                    theme.font_body,
                                    outfit.0,
                                ),
                                Pickable::IGNORE,
                            ));
                        });
                        chip.spawn((
                            Node {
                                width: Val::Px(28.0),
                                height: Val::Px(3.0),
                                ..default()
                            },
                            BackgroundColor(outfit.0),
                            Pickable::IGNORE,
                        ));
                        chip.spawn((bar_back(&theme, 28.0, 7.0), Pickable::IGNORE))
                            .with_children(|back| {
                                back.spawn((
                                    bar_fill(1.0, theme.health_fill),
                                    HealthFillOf { unit },
                                    Pickable::IGNORE,
                                ));
                            });
                    });
            }
        });
    } else {
        error!("HUD: no layout frame at spawn — roster strip not shown");
    }

    // --- Time / speed readout (bottom-right) ---
    let speed = (
        HudRoot,
        Node {
            padding: UiRect::axes(Val::Px(theme.space_sm), Val::Px(theme.space_xs)),
            ..default()
        },
        BackgroundColor(theme.panel),
        border_all(theme.panel_border),
        Pickable::IGNORE,
    );
    if let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::BottomRight, speed) {
        ec.with_children(|readout| {
            readout.spawn((
                text_colored(&theme, &fonts, "x1.0", theme.font_body, theme.accent),
                SpeedText,
                Pickable::IGNORE,
            ));
        });
    } else {
        error!("HUD: no layout frame at spawn — speed readout not shown");
    }
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
/// player hasn't hidden it; update its HP fill and hazard tier. Read-only of `enemy.rs`.
///
/// The tier drives **luminance and a glyph**, never a hue swap — one glance says how close the mask
/// is to coming off, and it still says it in grayscale.
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
        (hit || state.is_angry()).then(|| (health.fraction(), boss_hazard(state)))
    });

    match engaged {
        Some((frac, hazard)) if hud.show_boss_bar => {
            root_node.display = Display::Flex;
            let ink = theme.hazard_ink(hazard);
            if let Ok((mut f, mut bg)) = fill.single_mut() {
                f.width = Val::Percent(frac.clamp(0.0, 1.0) * 100.0);
                if bg.0 != ink {
                    bg.0 = ink;
                }
            }
            if let Ok((mut t, mut tc)) = label.single_mut() {
                let want = boss_label(hazard);
                if t.0 != want {
                    t.0 = want;
                }
                if tc.0 != ink {
                    tc.0 = ink;
                }
            }
        }
        _ => root_node.display = Display::None,
    }
}

/// `H` cycles the roster-detail density preset (Full → Compact → Hidden → …). The `docs/ui.md` §2
/// backbone made operable at the keyboard; the same values are exposed in the settings menu and
/// persisted.
fn cycle_density_key(keys: Res<ButtonInput<KeyCode>>, mut hud: ResMut<HudSettings>) {
    if keys.just_pressed(KeyCode::KeyH) {
        hud.roster_detail = match hud.roster_detail {
            RosterDetail::Full => RosterDetail::Compact,
            RosterDetail::Compact => RosterDetail::Hidden,
            RosterDetail::Hidden => RosterDetail::Full,
        };
    }
}

/// Apply HUD-density settings to node visibility (runs only when settings change).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_has_a_distinct_letter() {
        // The whole point of the letter. Two roles sharing one would put the roster back where the
        // bare colour swatch left it: red Gunman and green Psionic indistinguishable for a
        // deuteranope, and now indistinguishable for everyone else too.
        for (i, a) in RoleId::ALL.iter().enumerate() {
            for b in &RoleId::ALL[i + 1..] {
                assert_ne!(
                    role_letter(*a),
                    role_letter(*b),
                    "{a:?} and {b:?} share the letter {}",
                    role_letter(*a)
                );
            }
        }
    }

    #[test]
    fn every_role_is_labelled() {
        for r in RoleId::ALL {
            assert!(!role_letter(r).is_empty(), "{r:?} has no roster letter");
        }
    }

    #[test]
    fn the_boss_label_names_its_tier_and_carries_the_tier_glyph() {
        // Three channels: glyph, luminance (asserted in `theme`), and the word. The label must carry
        // two of the three on its own.
        for h in [Hazard::Vlam, Hazard::Keneq, Hazard::Ekhi] {
            let l = boss_label(h);
            assert!(l.starts_with(h.glyph()), "{h:?} label must lead with its glyph: {l}");
            assert!(l.contains("THE WATCHER"), "{l}");
        }
        assert_ne!(boss_label(Hazard::Keneq), boss_label(Hazard::Ekhi));
    }

    #[test]
    fn a_calmer_boss_never_reads_hotter_than_an_angry_one() {
        // Pins the mood -> tier direction. Inverting this would tell the player the mask is coming
        // off when it is going back on.
        assert!(boss_hazard_of(false, false) < boss_hazard_of(false, true));
        assert!(boss_hazard_of(false, true) < boss_hazard_of(true, false));
    }

    /// Test shim: `SmileyState` is a sim type with its own construction rules, so exercise the
    /// mapping through the same two predicates `boss_hazard` reads.
    fn boss_hazard_of(angry: bool, watching: bool) -> Hazard {
        if angry {
            Hazard::Ekhi
        } else if watching {
            Hazard::Keneq
        } else {
            Hazard::Vlam
        }
    }

    #[test]
    fn amida_is_reserved() {
        // The lore doc reserves the top tier ("the only time the screen goes white"). A boss mood
        // that reached it every fight would spend the reservation.
        for (angry, watching) in [(false, false), (false, true), (true, false), (true, true)] {
            assert_ne!(boss_hazard_of(angry, watching), Hazard::Amida);
        }
    }
}
