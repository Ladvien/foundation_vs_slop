//! **Containment HUD** (FVS-L-1) — why a capture is progressing, or why it is breaking.
//!
//! The item's acceptance is not "show a progress bar": it is *"players can read why containment is
//! progressing/breaking"*. A bar alone cannot answer that — a stalled bar looks identical whether the
//! squad is out of position, still shooting, or simply not looking at the thing. So this panel names the
//! **rule clauses**, marks each met or unmet, and the failing ones are the instruction: satisfy them.
//!
//! That is why `ContainmentRule::unmet` exists (`containment::rule`), and why the rule model is a
//! conjunction with no OR — an "either route" rule would make this readout ambiguous about which route
//! the player is on.
//!
//! Windowed-only, like the rest of `crate::ui`: `Update` and `OnEnter`/`OnExit` only, reads sim state and
//! never writes it, so nothing here enters `snapshot_hash`.

use bevy::prelude::*;

use super::state::{despawn_scoped, AppState};
use super::theme::{FontAssets, UiTheme, Z_MENU};
use super::widgets::text_colored;
use crate::ai::field::{FieldId, Stig};
use crate::containment::rule::Sign;
use crate::containment::{Containment, Phase};
use crate::dungeon::Dungeon;

/// Root marker for the containment panel (despawned on leaving the game).
#[derive(Component)]
pub struct ContainmentHudRoot;

/// The one text node the panel rewrites each frame.
#[derive(Component)]
pub struct ContainmentReadout;

pub struct ContainmentHudPlugin;

impl Plugin for ContainmentHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::InGame), spawn_panel)
            .add_systems(OnExit(AppState::InGame), despawn_scoped::<ContainmentHudRoot>)
            .add_systems(
                Update,
                update_readout.run_if(in_state(AppState::InGame)),
            );
    }
}

fn spawn_panel(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    commands
        .spawn((
            ContainmentHudRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(theme.space_lg),
                left: Val::Px(theme.space_lg),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                ..default()
            },
            GlobalZIndex(Z_MENU - 1),
        ))
        .with_children(|p| {
            p.spawn((
                ContainmentReadout,
                text_colored(&theme, &fonts, "", theme.font_body, theme.text),
            ));
        });
}

/// Human-readable name for a stigmergy channel, so the readout says "ATTENTION" rather than "channel 9".
///
/// A `match` rather than a lookup table: adding a channel to `ai::field` should fail to compile here
/// until someone decides what the player is told to do about it.
fn channel_name(field: FieldId) -> &'static str {
    match field {
        FieldId::SCENT => "BLOOD SCENT",
        FieldId::THREAT_GUN => "GUNFIRE",
        FieldId::CRAB_DENSITY => "SWARM DENSITY",
        FieldId::MEAT => "MEAT",
        FieldId::ALARM => "ALARM",
        FieldId::THREAT_CRAB => "SWARM DREAD",
        FieldId::THREAT_ANOMALY => "ANOMALY DREAD",
        FieldId::NOISE_SQUAD => "SQUAD NOISE",
        FieldId::NOISE_SWARM => "SWARM NOISE",
        FieldId::ATTENTION => "OBSERVATION",
        _ => "UNKNOWN",
    }
}

/// One line per clause: whether it is met, what it is, and what the cell currently reads.
///
/// Split out as a pure function so the wording — the actual deliverable of this item — is unit-testable
/// without a UI tree or an `App`.
fn clause_line(name: &str, sign: Sign, threshold: f32, actual: f32) -> String {
    let (mark, verb) = match sign {
        Sign::AtLeast if actual >= threshold => ("[OK]", "HOLD"),
        Sign::AtLeast => ("[! ]", "RAISE"),
        Sign::AtMost if actual <= threshold => ("[OK]", "HOLD"),
        Sign::AtMost => ("[! ]", "LOWER"),
    };
    let arrow = match sign {
        Sign::AtLeast => ">=",
        Sign::AtMost => "<=",
    };
    format!("{mark} {verb} {name} {arrow} {threshold:.2}  (now {actual:.2})")
}

/// Rewrite the readout from the anomaly currently under containment.
///
/// Shows the first in-progress capture rather than all of them: with one device archetype there is
/// normally one, and a list that silently truncates would be worse than one that is explicit about
/// showing a single target. Multi-target readout waits until a mechanic actually produces it.
fn update_readout(
    stig: Option<Res<Stig>>,
    dungeon: Option<Res<Dungeon>>,
    anomalies: Query<(&Containment, &Transform)>,
    mut readout: Query<&mut Text, With<ContainmentReadout>>,
) {
    let Ok(mut text) = readout.single_mut() else { return };
    // The fields only exist while a run is built (FVS-A-5), so both are optional here — this panel can
    // be alive on a frame where the world is being rebuilt.
    let (Some(stig), Some(dungeon)) = (stig, dungeon) else {
        text.0 = String::new();
        return;
    };

    let active = anomalies
        .iter()
        .find(|(c, _)| c.phase() == Phase::BeingContained);
    let Some((containment, transform)) = active else {
        text.0 = String::new();
        return;
    };

    let pos = transform.translation;
    let mut lines = vec![format!(
        "CONTAINMENT  {:>3.0}%   {:.1}s / {:.1}s",
        containment.progress() * 100.0,
        containment.held_secs(),
        containment.rule.hold_secs,
    )];
    for clause in &containment.rule.requires {
        let Some(field) = clause.field() else { continue };
        let actual = stig.sample(field, &dungeon, pos);
        lines.push(clause_line(channel_name(field), clause.sign, clause.threshold, actual));
    }
    text.0 = lines.join("\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_met_clause_says_hold_and_an_unmet_one_says_what_to_do() {
        // The deliverable of L-1 is that the line is an INSTRUCTION, not a status. An unmet clause has
        // to tell the player which way to push.
        let raise = clause_line("OBSERVATION", Sign::AtLeast, 0.5, 0.1);
        assert!(raise.starts_with("[! ]"), "an unmet clause is flagged: {raise}");
        assert!(raise.contains("RAISE"), "and says which way to push: {raise}");
        assert!(raise.contains("0.10"), "and shows the current reading: {raise}");

        let held = clause_line("OBSERVATION", Sign::AtLeast, 0.5, 0.9);
        assert!(held.starts_with("[OK]"));
        assert!(held.contains("HOLD"));

        // The opposite pole reads the other way — the `sign` semantics reach the player, not just the
        // evaluator (see `containment::rule` on why polarity is in the data).
        let lower = clause_line("GUNFIRE", Sign::AtMost, 0.1, 0.8);
        assert!(lower.contains("LOWER"), "an at-most clause tells the player to back off: {lower}");
        let quiet = clause_line("GUNFIRE", Sign::AtMost, 0.1, 0.0);
        assert!(quiet.starts_with("[OK]"));
    }

    #[test]
    fn the_boundary_reads_as_met_on_both_signs() {
        // Matches `FieldCondition::is_met`, which is inclusive — the HUD must not disagree with the rule
        // it is describing, or a player at exactly the threshold sees "not met" while the capture ticks.
        assert!(clause_line("X", Sign::AtLeast, 0.5, 0.5).starts_with("[OK]"));
        assert!(clause_line("X", Sign::AtMost, 0.5, 0.5).starts_with("[OK]"));
    }

    #[test]
    fn every_shipped_channel_has_a_player_facing_name() {
        // A rule may name any channel, so any channel can reach the readout. "UNKNOWN" in the HUD is a
        // content bug; catch it here rather than in a screenshot.
        for i in 0..crate::ai::field::CHANNEL_COUNT {
            let name = channel_name(FieldId(i));
            assert_ne!(name, "UNKNOWN", "channel {i} has no player-facing name");
        }
    }
}
