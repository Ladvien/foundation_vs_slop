//! **The verb bar** (FVS-B-3) — what the player can *do*, and what it costs.
//!
//! Push 2 shipped all three containment archetypes with no way to invoke any of them. This is the
//! readout for the input layer that fixes that (`crate::selection`): arm a verb — with its key or by
//! clicking its chip — then left-click the target.
//!
//! **Why a bar of named verbs rather than a passive stat block.** Vansteenkiste & Ryan 2013 (*On
//! psychological growth and vulnerability*, DOI 10.1037/a0032359) report that need-supportive
//! environments are the ones "that provide **meaningful choice** or deliver effectance-relevant
//! feedback … Conversely, **controlling reward contingencies**" undermine intrinsic motivation. A row
//! of distinct verbs with visible, spendable charges is choice plus effectance feedback; a hidden
//! multiplier is the other thing. It is the same argument FVS-F-2 makes for unlocks granting verbs
//! rather than numbers — which is why this bar is the surface those unlocks will later extend.
//!
//! # Why the chips are entities now
//!
//! The bar used to be one `Text` node holding `" C  DEVICE x3   [Z] QUARANTINE x1 <"`, with the armed
//! verb marked by ASCII brackets. Two costs. First, a string has no hit target: every verb was
//! keyboard-only, so a mouse-driven player faced a row of things that look like buttons and are not —
//! and Cook's *skill atom* (action → simulation → feedback → modeling) never closes if the obvious
//! action does nothing. Second, `[` and `<` are a weak mark to find while the player is looking at the
//! **world**; a chip carries a border and a luminance step, which are found preattentively.
//!
//! Clicking a chip does **not** write [`ArmedTool`]. It sends [`ArmRequest`], which
//! `selection::arm_tool_input` — the single writer — applies with exactly the same toggle rule as the
//! key, including the `DebugCaptureActive` stand-down. Click and key cannot drift apart.
//!
//! Windowed-only, like the rest of `crate::ui`: `Update` and `OnEnter`/`OnExit` only. It writes no sim
//! state; the one thing it emits is an input intent, which is what a key press is too.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use super::layout::{self, HudRegions, Region};
use super::state::{despawn_scoped, AppState};
use super::theme::{FontAssets, UiTheme};
use super::widgets::{border_all, text_colored};
use crate::containment::{ArmedTool, DeviceSupply, QuarantineSupply};
use crate::laser::WeaponsTight;
use crate::selection::ArmRequest;
use crate::session::{RunPhase, WinCondition};

/// Root marker for the bar (despawned on leaving the game).
#[derive(Component)]
pub struct VerbBarRoot;

/// The objective line above the bar.
#[derive(Component)]
pub struct ObjectiveReadout;

/// A verb chip, tagged so the styler can find the armed one.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub struct VerbChip(pub Verb);

/// A chip's label node, so charges can be rewritten without respawning the chip — which would drop
/// the hover state out from under a cursor that is mid-click.
#[derive(Component)]
pub struct VerbChipLabel(pub Verb);

/// The verbs the bar offers.
///
/// A closed enum rather than free strings, so a new verb cannot reach the player without someone
/// deciding its key, its wording, and the intent its chip sends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verb {
    Device,
    Quarantine,
    Cap,
    HoldFire,
}

impl Verb {
    pub const ALL: [Verb; 4] = [Verb::Device, Verb::Quarantine, Verb::Cap, Verb::HoldFire];

    /// Mirrors `selection::arm_tool_input`. `C`/`Z`/`X` are the free adjacent bottom-row cluster and
    /// `F` is free for fire discipline — see that function on why the choice is constrained.
    pub fn key(self) -> char {
        match self {
            Verb::Device => 'C',
            Verb::Quarantine => 'Z',
            Verb::Cap => 'X',
            Verb::HoldFire => 'F',
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Verb::Device => "DEVICE",
            Verb::Quarantine => "QUARANTINE",
            Verb::Cap => "CAP NEST",
            Verb::HoldFire => "HOLD FIRE",
        }
    }

    /// The intent a click on this chip sends.
    fn request(self) -> ArmRequest {
        match self {
            Verb::Device => ArmRequest::Toggle(ArmedTool::Device),
            Verb::Quarantine => ArmRequest::Toggle(ArmedTool::Quarantine),
            Verb::Cap => ArmRequest::Toggle(ArmedTool::Cap),
            Verb::HoldFire => ArmRequest::ToggleWeaponsTight,
        }
    }

    fn armed_by(self, armed: ArmedTool) -> bool {
        matches!(
            (self, armed),
            (Verb::Device, ArmedTool::Device)
                | (Verb::Quarantine, ArmedTool::Quarantine)
                | (Verb::Cap, ArmedTool::Cap)
        )
    }
}

pub struct VerbBarPlugin;

impl Plugin for VerbBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::InGame),
            spawn_bar.after(layout::spawn_frame),
        )
        .add_systems(OnExit(AppState::InGame), despawn_scoped::<VerbBarRoot>)
        .add_systems(
            Update,
            (update_chips, style_chips, update_objective).run_if(in_state(AppState::InGame)),
        );
    }
}

fn spawn_bar(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    regions: Res<HudRegions>,
) {
    // Bottom-centre, via the region grid.
    //
    // The old bar set `left: Val::Percent(50.0)` with `align_items: Center` and no width, which
    // centres the children *inside* an auto-width node whose LEFT EDGE sits at screen centre — so the
    // whole bar rendered offset right by half its own width. The region owns the centring now, so
    // there is no per-panel arithmetic left to get wrong.
    let root = (
        VerbBarRoot,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(theme.space_sm),
            ..default()
        },
        Pickable::IGNORE,
    );

    let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::BottomCenter, root) else {
        error!("verb bar: no layout frame at spawn — the player has no verb readout");
        return;
    };

    ec.with_children(|p| {
        p.spawn((
            ObjectiveReadout,
            text_colored(&theme, &fonts, "", theme.font_body, theme.accent),
            Pickable::IGNORE,
        ));

        p.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(theme.space_sm),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|bar| {
            for verb in Verb::ALL {
                bar.spawn((
                    VerbChip(verb),
                    // `bevy_ui_widgets::Button` (already plugged in by `DefaultPlugins`) emits
                    // `Activate` on release. The chip stays pickable — no `Pickable::IGNORE` — so
                    // clicks land on it rather than passing through to a move order.
                    Button,
                    Hovered::default(),
                    Node {
                        padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_xs)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(theme.radius)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    border_all(theme.panel_border),
                ))
                .observe(move |_: On<Activate>, mut arm: MessageWriter<ArmRequest>| {
                    arm.write(verb.request());
                })
                .with_children(|chip| {
                    chip.spawn((
                        VerbChipLabel(verb),
                        text_colored(&theme, &fonts, "", theme.font_body, theme.text),
                        Pickable::IGNORE,
                    ));
                });
            }
        });
    });
}

/// The text on one chip.
///
/// Pure, so the wording stays unit-testable without a UI tree. An exhausted verb still shows itself
/// at `x0` rather than disappearing — a player who has run out must learn *that*, not wonder where
/// the button went.
fn chip_label(verb: Verb, charges: Option<u32>, tight: bool) -> String {
    let key = verb.key();
    match verb {
        // Hold fire is a latched STANCE, not a spendable charge, so it reads on/off, never a count.
        Verb::HoldFire => {
            let mark = if tight { "  \u{2022}" } else { "" };
            format!("{key}  {}{mark}", verb.name())
        }
        _ => match charges {
            Some(n) => format!("{key}  {} x{n}", verb.name()),
            None => format!("{key}  {}", verb.name()),
        },
    }
}

fn charges_for(verb: Verb, devices: u32, quarantines: u32) -> Option<u32> {
    match verb {
        Verb::Device => Some(devices),
        Verb::Quarantine => Some(quarantines),
        Verb::Cap | Verb::HoldFire => None,
    }
}

fn update_chips(
    devices: Res<DeviceSupply>,
    quarantines: Res<QuarantineSupply>,
    tight: Res<WeaponsTight>,
    mut labels: Query<(&VerbChipLabel, &mut Text)>,
) {
    for (label, mut text) in &mut labels {
        let want = chip_label(label.0, charges_for(label.0, devices.0, quarantines.0), tight.0);
        if text.0 != want {
            text.0 = want;
        }
    }
}

/// Armed / hovered / exhausted styling.
///
/// The armed chip is marked by **luminance and border**, never by a hue change, so it is findable in
/// peripheral vision while the player looks at the world — the same encoding rule the rest of the HUD
/// follows (`docs/ui.md` §1.3). An exhausted verb dims rather than vanishing.
///
/// Runs every frame because `ArmedTool` changes are not per-entity change-detectable here and there
/// are four chips; the writes are still guarded so an unchanged chip does not churn its `Node`.
fn style_chips(
    theme: Res<UiTheme>,
    armed: Res<ArmedTool>,
    tight: Res<WeaponsTight>,
    devices: Res<DeviceSupply>,
    quarantines: Res<QuarantineSupply>,
    mut chips: Query<(&VerbChip, &Hovered, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<(&VerbChipLabel, &mut TextColor)>,
) {
    let lit = |verb: Verb| verb.armed_by(*armed) || (verb == Verb::HoldFire && tight.0);
    let spent = |verb: Verb| charges_for(verb, devices.0, quarantines.0) == Some(0);

    for (chip, hovered, mut bg, mut border) in &mut chips {
        let verb = chip.0;
        let want_bg = if lit(verb) {
            theme.panel_border.with_alpha(0.30)
        } else if hovered.0 {
            theme.panel_border.with_alpha(0.16)
        } else {
            theme.panel
        };
        let want_border = if lit(verb) {
            theme.accent
        } else if spent(verb) {
            theme.panel_border.with_alpha(0.25)
        } else {
            theme.panel_border
        };

        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want = border_all(want_border);
        if border.top != want.top {
            *border = want;
        }
    }

    for (label, mut color) in &mut labels {
        let verb = label.0;
        let want = if lit(verb) {
            theme.accent
        } else if spent(verb) {
            theme.text_muted
        } else {
            theme.text
        };
        if color.0 != want {
            color.0 = want;
        }
    }
}

/// What the player is supposed to be doing, derived from the phase.
///
/// Reads `RunPhase`, which is exactly what that state is for — presentation. It must never gate pinned
/// gameplay (see `session::advance_run_phase`), and an `Update`-side readout is the shape that cannot.
///
/// **This line must always name a next goal.** Phan, Keebler & Chaparro 2016's validation of the GUESS
/// (DOI 10.1177/0018720816669646, N=629) found *"I always know my next goal when I finish an event"*
/// the lowest-scoring item in the entire usability subscale (M=5.46 of 7) — the industry's weakest
/// link. So the instant the quota is met this switches to the extraction instruction instead of
/// continuing to report a count.
fn objective_line(
    win: WinCondition,
    phase: RunPhase,
    contained: u32,
    nests: (usize, usize),
) -> String {
    // Nest progress rides alongside the objective rather than in its own panel: capping is a verb with
    // no other feedback at all (`Capped` grants nothing and is deliberately invisible — FVS-B-7), so
    // without this the player seals a nest and sees literally nothing happen.
    let (capped, total) = nests;
    let sites = if total > 0 { format!("   NESTS {capped}/{total}") } else { String::new() };
    match win {
        WinCondition::SurviveTicks(_) => format!("HOLD THE SITE{sites}"),
        WinCondition::ExtractContained { count } => match phase {
            RunPhase::Locating => format!("LOCATE AND CONTAIN {count} ANOMALY(S){sites}"),
            RunPhase::Containing => format!("CONTAINING — {contained}/{count} SECURED{sites}"),
            RunPhase::Extracting => format!("RETURN TO THE EXTRACTION POINT{sites}"),
        },
    }
}

fn update_objective(
    win: Res<WinCondition>,
    phase: Res<State<RunPhase>>,
    secured: Res<crate::containment::SiteSecured>,
    contained: Query<(), With<crate::containment::Contained>>,
    mut readout: Query<&mut Text, With<ObjectiveReadout>>,
) {
    let line = objective_line(
        *win,
        *phase.get(),
        contained.iter().count() as u32,
        (secured.capped, secured.total),
    );
    for mut text in &mut readout {
        if text.0 != line {
            text.0 = line.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exhausted_verb_still_shows_itself_at_zero() {
        // Hiding it would teach the player the verb does not exist rather than that it is spent.
        assert!(chip_label(Verb::Device, Some(0), false).contains("DEVICE x0"));
        assert!(chip_label(Verb::Quarantine, Some(0), false).contains("QUARANTINE x0"));
    }

    #[test]
    fn hold_fire_reads_as_a_stance_not_a_charge() {
        let off = chip_label(Verb::HoldFire, None, false);
        let on = chip_label(Verb::HoldFire, None, true);
        assert!(off.contains("HOLD FIRE"));
        assert!(on.contains('\u{2022}'), "the latched stance is marked: {on}");
        assert_ne!(off, on);
        // Never a count — it is latched, not spent.
        assert!(!on.contains("HOLD FIRE x"));
    }

    #[test]
    fn every_verb_states_its_key() {
        // The chip is clickable AND keyed. A chip that did not name its key would teach the mouse
        // player that the keyboard route does not exist.
        for v in Verb::ALL {
            let l = chip_label(v, charges_for(v, 3, 1), false);
            assert!(l.starts_with(v.key()), "{v:?} chip must lead with its key: {l}");
        }
    }

    #[test]
    fn verb_keys_are_unique() {
        // Two verbs on one key would make one of them permanently unreachable from the keyboard.
        for (i, a) in Verb::ALL.iter().enumerate() {
            for b in &Verb::ALL[i + 1..] {
                assert_ne!(a.key(), b.key(), "{a:?} and {b:?} share key {}", a.key());
            }
        }
    }

    #[test]
    fn a_click_sends_the_same_intent_the_key_does() {
        // Pins the mapping `selection::arm_tool_input` applies, so a new verb cannot get a chip
        // without getting the matching arm request — which is how the click and the key stay one
        // behaviour with one writer instead of two that drift.
        assert_eq!(Verb::Device.request(), ArmRequest::Toggle(ArmedTool::Device));
        assert_eq!(Verb::Quarantine.request(), ArmRequest::Toggle(ArmedTool::Quarantine));
        assert_eq!(Verb::Cap.request(), ArmRequest::Toggle(ArmedTool::Cap));
        assert_eq!(Verb::HoldFire.request(), ArmRequest::ToggleWeaponsTight);
    }

    #[test]
    fn only_the_armed_verb_reads_as_armed() {
        assert!(Verb::Device.armed_by(ArmedTool::Device));
        assert!(!Verb::Quarantine.armed_by(ArmedTool::Device));
        for v in Verb::ALL {
            assert!(!v.armed_by(ArmedTool::None), "{v:?} must not read as armed when nothing is");
        }
        // Hold fire is a stance, not an `ArmedTool`, so it is never "armed" by this path.
        assert!(!Verb::HoldFire.armed_by(ArmedTool::Cap));
    }

    #[test]
    fn the_objective_names_the_extraction_only_once_the_quota_is_met() {
        let win = WinCondition::ExtractContained { count: 1 };
        assert!(objective_line(win, RunPhase::Locating, 0, (0, 0)).contains("LOCATE"));
        assert!(objective_line(win, RunPhase::Containing, 0, (0, 0)).contains("0/1"));
        assert!(objective_line(win, RunPhase::Extracting, 1, (0, 0)).contains("EXTRACTION"));
    }

    #[test]
    fn the_objective_is_never_blank_in_any_phase() {
        // GUESS's weakest item is "I always know my next goal when I finish an event". A blank
        // objective line is that failure in its purest form.
        for win in [
            WinCondition::SurviveTicks(100),
            WinCondition::ExtractContained { count: 2 },
        ] {
            for phase in [RunPhase::Locating, RunPhase::Containing, RunPhase::Extracting] {
                let l = objective_line(win, phase, 0, (1, 3));
                assert!(!l.trim().is_empty(), "{win:?}/{phase:?} left the player with no goal");
            }
        }
    }

    #[test]
    fn nest_progress_shows_only_when_there_are_nests() {
        // A "NESTS 0/0" on a level with none would be noise, and capping is otherwise INVISIBLE
        // feedback (`Capped` grants nothing by design), so this line is the verb's only acknowledgement.
        let win = WinCondition::ExtractContained { count: 1 };
        assert!(!objective_line(win, RunPhase::Locating, 0, (0, 0)).contains("NESTS"));
        assert!(objective_line(win, RunPhase::Locating, 0, (2, 4)).contains("NESTS 2/4"));
    }

    #[test]
    fn the_placeholder_win_never_tells_the_player_to_extract() {
        // `SurviveTicks` has no quota, so an "extract" instruction would be a lie. Pins the pairing
        // between the win variant and the copy.
        let win = WinCondition::SurviveTicks(100);
        for phase in [RunPhase::Locating, RunPhase::Containing, RunPhase::Extracting] {
            assert_eq!(objective_line(win, phase, 0, (0, 0)), "HOLD THE SITE");
        }
    }
}
