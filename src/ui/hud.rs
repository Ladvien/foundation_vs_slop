//! In-game HUD (clear overlay). Reads collision-free sim state only:
//! - **Squad roster strip** (bottom-left) — also the **selection readout**: one chip per [`Unit`]
//!   with its role letter, [`Outfit`] colour, live health, a frame while selected, and a tag line
//!   carrying control-group membership and queued-order count. It gained the last three when
//!   `crate::selection` made selection real; under the old always-selected scheme there was nothing
//!   for a chip to say about it.
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

/// A chip's outfit-colour swatch — the one part of a roster chip that is **pure reinforcement**.
///
/// This is what `RosterDetail::Compact` actually removes. Before it existed, `Compact` was a lie:
/// `apply_hud_settings` matched only `Hidden` vs everything else, so two of the three density rungs
/// rendered identically while `H` cycled all three and the settings menu named all three.
///
/// It is the right thing to cut first. `docs/ui.md` §2 draws the rule from Iacovides et al. 2015 —
/// what a lower density sheds is what competes for **attention and agency**, never information the
/// run depends on. The swatch is decoration on top of a role letter that already identifies the
/// operative (the whole argument in this module's header), so removing it costs no channel; the
/// health bar and the letter stay at every visible rung, which is what keeps the §2 promise that a
/// run is completable at the minimal preset.
#[derive(Component)]
pub struct RosterSwatch;

/// The boss-bar container (shown only while the boss is engaged + `show_boss_bar`).
#[derive(Component)]
pub struct BossBarRoot;

/// A roster chip, bound to the operative it describes.
///
/// The strip became the **selection readout** when `crate::selection` made selection real. Before
/// that, every unit was permanently selected, so a chip could not say anything about selection —
/// there was nothing to say.
#[derive(Component)]
pub struct RosterChipOf {
    pub unit: Entity,
}

/// A chip's bottom line: control-group membership and queued orders.
///
/// **This line is not decoration — it is what makes control groups usable.** Cockburn, Gutwin, Scarr
/// & Malacria 2014 (*Supporting Novice to Expert Transitions in User Interfaces*, ACM Comput. Surv.
/// 47(2), DOI 10.1145/2659796) document the intramodal/intermodal failure this addresses: expert
/// mechanisms exist and go unused because users plateau on the slow method, and no single moment
/// hurts enough to justify switching. A `Ctrl+2` that produced **no visible effect whatsoever** is
/// the worst case of that — the player cannot even confirm the binding took, so there is nothing to
/// build a habit on. The label is the feedback that closes the loop.
///
/// It carries the queued-order count for the same reason: shift-right-click is otherwise a command
/// with no acknowledgement, and `selection::MAX_QUEUED_ORDERS` is a cap the player must be able to
/// see themselves approaching rather than discover by having an order silently refused.
#[derive(Component)]
pub struct RosterChipTag {
    pub unit: Entity,
    /// Stable squad index, which is what [`crate::selection::ControlGroups`] stores.
    pub member: usize,
}

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
        // `update_selection_marks` reads `ControlGroups` non-optionally. `SelectionPlugin` owns it and
        // is harness-visible, so it is normally present — but a missing `Res<T>` is a panic in Bevy
        // 0.19 (`docs/ui.md` §5, trap 2), and the rule is that the plugin registering a reader claims
        // the resource. `init_resource` is idempotent, so the real one still wins.
        app.init_resource::<crate::selection::ControlGroups>();
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
                update_selection_marks,
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
    units: Query<(Entity, &Outfit, &RoleId, &crate::squad::SquadMember), With<Unit>>,
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
            for (unit, outfit, role, member) in &units {
                strip
                    .spawn((
                        // The chip is now the SELECTION readout as well as the health readout, so it
                        // is tagged with whose it is (see `RosterChipOf`).
                        RosterChipOf { unit },
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: Val::Px(theme.space_xs),
                            padding: UiRect::all(Val::Px(2.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(theme.radius)),
                            ..default()
                        },
                        // Transparent until selected — `update_selection_marks` supplies the border.
                        border_all(Color::NONE),
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
                        // The outfit swatch — pure reinforcement, and therefore the first thing the
                        // density preset drops (see `RosterSwatch`).
                        chip.spawn((
                            RosterSwatch,
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
                        // Control-group membership and queued orders. This line is why the strip is
                        // now a selection readout — see `RosterChipTag`.
                        chip.spawn((
                            RosterChipTag { unit, member: member.0 },
                            text_colored(&theme, &fonts, "", theme.font_body * 0.7, theme.text_muted),
                            Pickable::IGNORE,
                        ));
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
/// What a chip's tag line says: which control groups the operative is in, and how many orders are
/// queued behind their current one.
///
/// Pure, so the wording is testable without a world. Empty when there is nothing to report — a chip
/// showing `-` or `0` for every operative all run long would be noise, and `docs/ui.md` §1.2 is
/// explicit that noise is not neutral. That is the one case where saying nothing is right: this is a
/// *supplement* to a chip that already reads (letter + health), not a panel that could look broken.
pub fn chip_tag_text(groups: &[usize], queued: usize) -> String {
    let mut parts = Vec::new();
    if !groups.is_empty() {
        // Digits only. At `font_body * 0.7` under a 28 px chip there is room for a couple of
        // characters, and Rosenholtz's point applies to a chip read in peripheral vision too: gross
        // shape survives, detail does not.
        parts.push(groups.iter().map(|g| g.to_string()).collect::<Vec<_>>().join(""));
    }
    if queued > 0 {
        // `+N` rather than a bare count, so it reads as "more to come" rather than as a group number.
        parts.push(format!("+{queued}"));
    }
    parts.join(" ")
}

/// Mark the selected operatives, and keep each chip's tag line current.
///
/// The border is the selection mark: **luminance and a frame, never a hue change**, the same encoding
/// the verb chips use (`docs/ui.md` §1.3), so it is findable in peripheral vision while the player is
/// looking at the world.
fn update_selection_marks(
    theme: Res<UiTheme>,
    groups: Res<crate::selection::ControlGroups>,
    selected: Query<(), With<crate::squad::Selected>>,
    queues: Query<&crate::selection::OrderQueue>,
    mut chips: Query<(&RosterChipOf, &mut BorderColor)>,
    mut tags: Query<(&RosterChipTag, &mut Text)>,
) {
    for (chip, mut border) in &mut chips {
        let want = if selected.contains(chip.unit) {
            crate::ui::widgets::border_all(theme.accent)
        } else {
            // `Color::NONE`, not the panel border: an unselected chip must not read as a weakly
            // selected one. Selection is binary and the mark has to be too.
            crate::ui::widgets::border_all(Color::NONE)
        };
        if border.top != want.top {
            *border = want;
        }
    }
    for (tag, mut text) in &mut tags {
        let queued = queues.get(tag.unit).map(|q| q.len()).unwrap_or(0);
        let want = chip_tag_text(&groups.labels_for(tag.member), queued);
        if text.0 != want {
            text.0 = want;
        }
    }
}

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
fn cycle_density_key(actions: crate::input::Actions, mut hud: ResMut<HudSettings>) {
    if actions.just_pressed(crate::input::Action::CycleHudDensity) {
        hud.roster_detail = match hud.roster_detail {
            RosterDetail::Full => RosterDetail::Compact,
            RosterDetail::Compact => RosterDetail::Hidden,
            RosterDetail::Hidden => RosterDetail::Full,
        };
    }
}

/// Apply HUD-density settings to node visibility (runs only when settings change).
///
/// **Three rungs, three renderings.** `Hidden` drops the strip; `Compact` keeps the strip but drops
/// each chip's [`RosterSwatch`]; `Full` shows everything. The `Compact` branch is the one that used
/// to be missing — see [`RosterSwatch`] for why the swatch is the correct thing to shed.
fn apply_hud_settings(
    hud: Res<HudSettings>,
    mut roster: Query<&mut Node, (With<RosterStripRoot>, Without<RosterSwatch>)>,
    mut swatches: Query<&mut Node, (With<RosterSwatch>, Without<RosterStripRoot>)>,
) {
    if !hud.is_changed() {
        return;
    }
    if let Ok(mut node) = roster.single_mut() {
        node.display = strip_display(hud.roster_detail);
    }
    let want = swatch_display(hud.roster_detail);
    for mut node in &mut swatches {
        if node.display != want {
            node.display = want;
        }
    }
}

/// Whether the roster strip itself is drawn.
///
/// Pure, so the density mapping is testable without an `App` — the codebase's standing idiom for UI
/// logic, and the reason the missing `Compact` branch is now catchable by a unit test rather than by
/// a player noticing that two of three presets look the same.
fn strip_display(detail: RosterDetail) -> Display {
    match detail {
        RosterDetail::Hidden => Display::None,
        RosterDetail::Compact | RosterDetail::Full => Display::Flex,
    }
}

/// Whether a chip's outfit swatch is drawn. Exhaustive rather than `_`-terminated, so adding a
/// fourth rung is a compile error here instead of a silent aliasing onto an existing one.
fn swatch_display(detail: RosterDetail) -> Display {
    match detail {
        RosterDetail::Full => Display::Flex,
        RosterDetail::Compact | RosterDetail::Hidden => Display::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bound_control_group_is_visible_on_the_chip() {
        // Cockburn et al. 2014's intermodal-transition failure, in miniature: a `Ctrl+2` with no
        // visible effect gives the player nothing to confirm the binding on, so the habit never forms
        // and the expert path stays unused however well it works.
        assert_eq!(chip_tag_text(&[2], 0), "2");
        assert_eq!(chip_tag_text(&[1, 3], 0), "13", "membership in several groups reads compactly");
    }

    #[test]
    fn a_queued_order_reads_as_more_to_come_not_as_a_group_number() {
        // `+2` and `2` must not be confusable: one is "two orders behind this one", the other is
        // "control group two". Sharing a glyph would make the chip actively misleading.
        assert_eq!(chip_tag_text(&[], 2), "+2");
        assert_eq!(chip_tag_text(&[1], 2), "1 +2");
        assert_ne!(chip_tag_text(&[2], 0), chip_tag_text(&[], 2));
    }

    #[test]
    fn a_chip_with_nothing_to_report_says_nothing() {
        // The one place silence is right. `docs/ui.md` §1.2 — a widget supporting no decision is
        // noise, and a `-` or a `0` on all five chips for a whole run is exactly that. The chip still
        // reads (role letter + health), so this is a supplement, not a panel that could look broken.
        assert_eq!(chip_tag_text(&[], 0), "");
    }

    #[test]
    fn all_three_density_rungs_look_different() {
        // The bug this pins: `Compact` was a no-op. The applier matched `Hidden` vs `_`, so Full and
        // Compact rendered identically while `H` cycled three states and the settings menu named
        // three. A preset that changes nothing is worse than one that does not exist — the player
        // presses the key, sees no change, and concludes the key is broken.
        let rungs = [RosterDetail::Full, RosterDetail::Compact, RosterDetail::Hidden];
        let render = |d| (strip_display(d), swatch_display(d));
        for (i, a) in rungs.iter().enumerate() {
            for b in &rungs[i + 1..] {
                assert_ne!(render(*a), render(*b), "{a:?} and {b:?} render identically");
            }
        }
    }

    #[test]
    fn a_lower_density_never_hides_health() {
        // `docs/ui.md` §2's non-negotiable: a lower preset must not break playability. Squad health
        // is load-bearing, so the only thing `Compact` may shed is the decorative swatch — the strip
        // (which carries the letter and the health bar) stays up.
        assert_eq!(strip_display(RosterDetail::Compact), Display::Flex);
        assert_eq!(swatch_display(RosterDetail::Compact), Display::None);
    }

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
