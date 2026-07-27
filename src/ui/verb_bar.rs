//! **The verb bar** (FVS-B-3) — what the player can *do*, and what it costs.
//!
//! Push 2 shipped all three containment archetypes with no way to invoke any of them. This is the
//! readout for the input layer that fixes that (`crate::selection`): arm a verb with a key, then
//! left-click the target.
//!
//! **Why a bar of named verbs rather than a passive stat block.** Vansteenkiste & Ryan 2013 (*On
//! psychological growth and vulnerability*, DOI 10.1037/a0032359) report that need-supportive
//! environments are the ones "that provide **meaningful choice** or deliver effectance-relevant
//! feedback … Conversely, **controlling reward contingencies**" undermine intrinsic motivation. A row
//! of distinct verbs with visible, spendable charges is choice plus effectance feedback; a hidden
//! multiplier is the other thing. It is the same argument FVS-F-2 makes for unlocks granting verbs
//! rather than numbers — which is why this bar is the surface those unlocks will later extend.
//!
//! Windowed-only, like the rest of `crate::ui`: `Update` and `OnEnter`/`OnExit` only, reads state and
//! never writes it, so nothing here can enter `snapshot_hash`.

use bevy::prelude::*;

use super::state::{despawn_scoped, AppState};
use super::theme::{FontAssets, UiTheme, Z_MENU};
use super::widgets::text_colored;
use crate::containment::{ArmedTool, DeviceSupply, QuarantineSupply};
use crate::laser::WeaponsTight;
use crate::session::{RunPhase, WinCondition};

/// Root marker for the bar (despawned on leaving the game).
#[derive(Component)]
pub struct VerbBarRoot;

/// The one text node the bar rewrites each frame.
#[derive(Component)]
pub struct VerbBarReadout;

/// The objective line above the bar.
#[derive(Component)]
pub struct ObjectiveReadout;

pub struct VerbBarPlugin;

impl Plugin for VerbBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::InGame), spawn_bar)
            .add_systems(OnExit(AppState::InGame), despawn_scoped::<VerbBarRoot>)
            .add_systems(
                Update,
                (update_bar, update_objective).run_if(in_state(AppState::InGame)),
            );
    }
}

fn spawn_bar(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    commands
        .spawn((
            VerbBarRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(theme.space_lg),
                left: Val::Percent(50.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(theme.space_xs),
                ..default()
            },
            GlobalZIndex(Z_MENU - 1),
        ))
        .with_children(|p| {
            p.spawn((
                ObjectiveReadout,
                text_colored(&theme, &fonts, "", theme.font_body, theme.accent),
            ));
            p.spawn((
                VerbBarReadout,
                text_colored(&theme, &fonts, "", theme.font_body, theme.text),
            ));
        });
}

/// Render one verb chip.
///
/// Pure, so the wording is unit-testable without a UI tree or an `App` — the same reason
/// `containment_hud::clause_line` is split out.
///
/// The armed verb is bracketed rather than merely recoloured: the bar has to read at a glance while the
/// player is looking at the *world*, and an exhausted verb must be distinguishable from an unavailable
/// one (`x0` rather than hidden), so a player who has run out learns *that* rather than wondering where
/// the button went.
fn verb_chip(key: char, label: &str, charges: Option<u32>, armed: bool) -> String {
    let count = match charges {
        Some(n) => format!(" x{n}"),
        None => String::new(),
    };
    if armed {
        format!("[{key}] {label}{count} <")
    } else {
        format!(" {key}  {label}{count} ")
    }
}

/// The whole bar, as one line.
fn bar_line(
    armed: ArmedTool,
    devices: u32,
    quarantines: u32,
    tight: bool,
) -> String {
    let mut s = String::new();
    s.push_str(&verb_chip('C', "DEVICE", Some(devices), armed == ArmedTool::Device));
    s.push_str("  ");
    s.push_str(&verb_chip('Z', "QUARANTINE", Some(quarantines), armed == ArmedTool::Quarantine));
    s.push_str("  ");
    s.push_str(&verb_chip('X', "CAP NEST", None, armed == ArmedTool::Cap));
    s.push_str("  ");
    // Hold fire is a latched STANCE, not a spendable charge, so it reads as on/off rather than a count.
    s.push_str(&verb_chip('F', if tight { "HOLD FIRE •" } else { "HOLD FIRE" }, None, false));
    s
}

/// What the player is supposed to be doing, derived from the phase.
///
/// Reads `RunPhase`, which is exactly what that state is for — presentation. It must never gate pinned
/// gameplay (see `session::advance_run_phase`), and a `Update`-side readout is the shape that cannot.
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

fn update_bar(
    armed: Res<ArmedTool>,
    devices: Res<DeviceSupply>,
    quarantines: Res<QuarantineSupply>,
    tight: Res<WeaponsTight>,
    mut readout: Query<&mut Text, With<VerbBarReadout>>,
) {
    let line = bar_line(*armed, devices.0, quarantines.0, tight.0);
    for mut text in &mut readout {
        if text.0 != line {
            text.0 = line.clone();
        }
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
    fn the_armed_verb_is_the_only_one_marked() {
        let line = bar_line(ArmedTool::Device, 3, 1, false);
        assert!(line.contains("[C] DEVICE x3 <"), "the armed verb is bracketed: {line}");
        assert!(line.contains(" Z  QUARANTINE x1 "), "an unarmed verb is not: {line}");
    }

    #[test]
    fn an_exhausted_verb_still_shows_itself_at_zero() {
        // Hiding it would teach the player the verb does not exist rather than that it is spent.
        let line = bar_line(ArmedTool::None, 0, 0, false);
        assert!(line.contains("DEVICE x0"), "{line}");
        assert!(line.contains("QUARANTINE x0"), "{line}");
    }

    #[test]
    fn hold_fire_reads_as_a_stance_not_a_charge() {
        assert!(bar_line(ArmedTool::None, 1, 1, false).contains("HOLD FIRE "));
        assert!(bar_line(ArmedTool::None, 1, 1, true).contains("HOLD FIRE •"));
        // Never a count — it is latched, not spent.
        assert!(!bar_line(ArmedTool::None, 1, 1, true).contains("HOLD FIRE x"));
    }

    #[test]
    fn the_objective_names_the_extraction_only_once_the_quota_is_met() {
        let win = WinCondition::ExtractContained { count: 1 };
        assert!(objective_line(win, RunPhase::Locating, 0, (0, 0)).contains("LOCATE"));
        assert!(objective_line(win, RunPhase::Containing, 0, (0, 0)).contains("0/1"));
        assert!(objective_line(win, RunPhase::Extracting, 1, (0, 0)).contains("EXTRACTION"));
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
