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
//! # Why the clauses are rows, not a string
//!
//! The acceptance says the player can **read why**. When the whole panel was one `Text` node it had
//! one `TextColor`, so a met clause and the unmet clause the player must act on rendered in
//! identical ink — the actionable line was findable only by *reading* the `[! ]` marker, not by
//! seeing it. Rows carry their own emphasis (`crate::ui::rows`), so the unmet clause is the
//! brightest thing in the panel and lands in peripheral vision while the player is looking at the
//! world. Emphasis is luminance, never hue, so this holds for red-green colour vision deficiency
//! too. The instruction-not-status copy rule is unchanged and still tested.
//!
//! Windowed-only, like the rest of `crate::ui`: `Update` and `OnEnter`/`OnExit` only, reads sim state and
//! never writes it, so nothing here enters `snapshot_hash`.

use bevy::prelude::*;

use super::layout::{self, HudRegions, Region};
use super::rows::{sync_rows, Cell, Row, RowPanel};
use super::state::{despawn_scoped, AppState};
use super::theme::{FontAssets, UiTheme};
use crate::ai::field::{FieldId, Stig};
use crate::containment::rule::Sign;
use crate::containment::{Containment, Phase};
use crate::dungeon::Dungeon;

/// Root marker for the containment panel (despawned on leaving the game).
#[derive(Component)]
pub struct ContainmentHudRoot;

/// The node whose children are the readout's rows.
#[derive(Component)]
pub struct ContainmentReadout;

pub struct ContainmentHudPlugin;

impl Plugin for ContainmentHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::InGame),
            // The panel is parented into a layout region, so the frame has to exist first.
            spawn_panel.after(layout::spawn_frame),
        )
        .add_systems(OnExit(AppState::InGame), despawn_scoped::<ContainmentHudRoot>)
        .add_systems(
            Update,
            update_readout.run_if(in_state(AppState::InGame)).distributive_run_if(in_state(crate::session::RunState::Active)),
        );
    }
}

fn spawn_panel(mut commands: Commands, theme: Res<UiTheme>, regions: Res<HudRegions>) {
    // Bottom-left, in the region grid rather than at a hand-picked absolute offset. This panel and
    // the roster strip both used to anchor bottom-left with different paddings and drew over each
    // other; now they are siblings in one flex column and stack.
    let panel = (
        ContainmentHudRoot,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(theme.space_xs),
            padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_sm)),
            // A minimum so the rows do not re-flow the panel's width every time a reading ticks
            // from "0.09" to "0.10" — a box that twitches while the player reads it is worse than
            // a slightly wide one.
            min_width: Val::Px(300.0),
            ..default()
        },
        BackgroundColor(theme.panel),
        super::widgets::border_all(theme.panel_border),
        // Non-interactive: it is a readout, and swallowing clicks here would eat move orders.
        Pickable::IGNORE,
    );

    let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::BottomLeft, panel) else {
        // No frame means no screen to attach to. Loud, because a silently missing containment
        // readout is the one panel the run's core loop depends on.
        error!("containment HUD: no layout frame at spawn — panel not shown");
        return;
    };
    ec.with_children(|p| {
        p.spawn((
            ContainmentReadout,
            RowPanel::default(),
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
            Pickable::IGNORE,
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

/// One row per clause: whether it is met, what to do about it, and what the cell currently reads.
///
/// Pure, so the wording — the actual deliverable of this item — stays unit-testable without a UI
/// tree or an `App`. It now returns a [`Row`] rather than a `String`, which is what lets the unmet
/// clause be *brighter* than the met ones instead of merely differently punctuated.
///
/// The boundary is **inclusive on both signs**, matching `FieldCondition::is_met`: a player sitting
/// exactly on the threshold must never read "not met" while the capture ticks up.
fn clause_row(name: &str, sign: Sign, threshold: f32, actual: f32) -> Row {
    let met = match sign {
        Sign::AtLeast => actual >= threshold,
        Sign::AtMost => actual <= threshold,
    };
    // The verb is the whole point: an unmet clause is an INSTRUCTION ("RAISE OBSERVATION"), never a
    // status ("observation: unmet"). A bare bool could not say which way to push, which is why
    // polarity lives in the data.
    let verb = match (sign, met) {
        (_, true) => "HOLD",
        (Sign::AtLeast, false) => "RAISE",
        (Sign::AtMost, false) => "LOWER",
    };
    let bound = match sign {
        Sign::AtLeast => "\u{2265}", // ≥
        Sign::AtMost => "\u{2264}",  // ≤
    };
    let label = format!("{verb} {name}");
    let value = format!("{bound} {threshold:.2}   now {actual:.2}");

    let row = if met { Row::met(label, value) } else { Row::unmet(label, value) };
    // A bar for the *approach* to the threshold. Length, not colour — it is the most accurately
    // read encoding, and it answers "how close am I" without the player parsing two decimals.
    row.push(Cell::Bar { frac: clause_progress(sign, threshold, actual) })
}

/// How satisfied a clause is, in `[0, 1]`. `1.0` means met.
///
/// Separate and pure because the two signs approach their threshold from opposite directions, and
/// getting that backwards would draw a full bar for the worst possible reading.
fn clause_progress(sign: Sign, threshold: f32, actual: f32) -> f32 {
    match sign {
        // Climbing toward the threshold.
        Sign::AtLeast => {
            if threshold <= 0.0 {
                1.0
            } else {
                (actual / threshold).clamp(0.0, 1.0)
            }
        }
        // Already satisfied at zero; falls off as the reading climbs past the threshold. Measured
        // against a full unit scale so the bar keeps moving above the bound instead of pinning.
        Sign::AtMost => {
            if actual <= threshold {
                1.0
            } else {
                let span = (1.0 - threshold).max(f32::EPSILON);
                (1.0 - (actual - threshold) / span).clamp(0.0, 1.0)
            }
        }
    }
}

/// Rewrite the readout from the anomaly currently under containment.
///
/// Shows the first in-progress capture rather than all of them: with one device archetype there is
/// normally one, and a list that silently truncates would be worse than one that is explicit about
/// showing a single target. Multi-target readout waits until a mechanic actually produces it.
/// The whole panel, as rows.
///
/// Pure and `App`-free: `legible` is passed in already resolved rather than the query, so the
/// panel's *content* stays testable while the ECS plumbing stays in [`update_readout`].
fn readout_rows(
    progress: f32,
    held_secs: f32,
    hold_secs: f32,
    clauses: &[(&str, Sign, f32, f32)],
    legible: bool,
) -> Vec<Row> {
    let mut rows = vec![
        Row::header("CONTAINMENT"),
        Row::kv(
            format!("{:.0}%", progress * 100.0),
            format!("{held_secs:.1}s / {hold_secs:.1}s"),
        )
        .push(Cell::Bar { frac: progress }),
    ];
    if legible {
        for (name, sign, threshold, actual) in clauses {
            rows.push(clause_row(name, *sign, *threshold, *actual));
        }
    } else {
        // Not a blank panel: an empty list reads as a bug, and the player needs to know that the
        // missing information is *obtainable* rather than absent. Same rule as every other readout
        // here — state the route, do not just refuse.
        rows.push(Row::note(
            "PROCEDURE UNKNOWN — NO OPERATIVE HAS STUDIED THIS ANOMALY",
        ));
    }
    rows
}

fn update_readout(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    stig: Option<Res<Stig>>,
    dungeon: Option<Res<Dungeon>>,
    rules: Option<Res<crate::containment::ContainmentRules>>,
    squad: Query<&crate::knowledge::Knowledge, With<crate::squad::Unit>>,
    anomalies: Query<(&Containment, &Transform)>,
    mut readout: Query<(Entity, &mut RowPanel), With<ContainmentReadout>>,
) {
    let Ok((entity, mut panel)) = readout.single_mut() else { return };
    let clear = |commands: &mut Commands, panel: &mut RowPanel| {
        sync_rows(commands, entity, panel, &theme, &fonts, Vec::new());
    };

    // The fields only exist while a run is built (FVS-A-5), so both are optional here — this panel can
    // be alive on a frame where the world is being rebuilt.
    let (Some(stig), Some(dungeon)) = (stig, dungeon) else {
        clear(&mut commands, &mut panel);
        return;
    };

    // Shows the first in-progress capture rather than all of them: with one device archetype there
    // is normally one, and a list that silently truncates would be worse than one that is explicit
    // about showing a single target. Multi-target readout waits until a mechanic produces it.
    //
    // SORT-OK: `find` over a single-element-in-practice set, and the panel is presentation only —
    // it reads sim state and writes nothing hashed, so a tie here cannot move `snapshot_hash`.
    let active = anomalies
        .iter()
        .find(|(c, _)| c.phase() == Phase::BeingContained);
    let Some((containment, transform)) = active else {
        clear(&mut commands, &mut panel);
        return;
    };

    let pos = transform.translation;
    // FVS-O-2's **benefit** half: knowledge is what makes a containment procedure legible. Gated behind
    // `containment.require_knowledge_for_rules` — see that field for why turning it on is a pacing
    // decision rather than a wiring one.
    //
    // "Any operative present knows" rather than "the selected one": this panel is the player's view of
    // the *squad*, and a squad that contains someone who has read the write-up would say so out loud.
    let gated = rules.map(|r| r.0.require_knowledge_for_rules).unwrap_or(false);
    let legible = !gated || squad.iter().any(|k| k.can_read_rule(containment.subject));

    let clauses: Vec<(&str, Sign, f32, f32)> = containment
        .rule
        .requires
        .iter()
        .filter_map(|clause| {
            let field = clause.field()?;
            Some((
                channel_name(field),
                clause.sign,
                clause.threshold,
                stig.sample(field, &dungeon, pos),
            ))
        })
        .collect();

    let rows = readout_rows(
        containment.progress(),
        containment.held_secs(),
        containment.rule.hold_secs,
        &clauses,
        legible,
    );
    sync_rows(&mut commands, entity, &mut panel, &theme, &fonts, rows);
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::rows::Emphasis;

    /// The clause label, which is the instruction the player acts on.
    fn label_of(r: &Row) -> String {
        r.label().unwrap_or_default().to_string()
    }

    #[test]
    fn a_met_clause_says_hold_and_an_unmet_one_says_what_to_do() {
        // The deliverable of L-1 is that the line is an INSTRUCTION, not a status. An unmet clause has
        // to tell the player which way to push.
        let raise = clause_row("OBSERVATION", Sign::AtLeast, 0.5, 0.1);
        assert_eq!(raise.emphasis, Emphasis::Alert, "an unmet clause is the loud row");
        assert!(label_of(&raise).starts_with("RAISE"), "and says which way to push: {raise:?}");

        let held = clause_row("OBSERVATION", Sign::AtLeast, 0.5, 0.9);
        assert_eq!(held.emphasis, Emphasis::Muted, "a met clause recedes");
        assert!(label_of(&held).starts_with("HOLD"));

        // The opposite pole reads the other way — the `sign` semantics reach the player, not just the
        // evaluator (see `containment::rule` on why polarity is in the data).
        let lower = clause_row("GUNFIRE", Sign::AtMost, 0.1, 0.8);
        assert!(label_of(&lower).starts_with("LOWER"), "an at-most clause tells the player to back off");
        assert_eq!(clause_row("GUNFIRE", Sign::AtMost, 0.1, 0.0).emphasis, Emphasis::Muted);
    }

    #[test]
    fn a_clause_shows_the_current_reading_next_to_the_threshold() {
        // The reading is what makes the instruction actionable — "RAISE OBSERVATION" with no number
        // cannot tell the player whether they are close.
        let r = clause_row("OBSERVATION", Sign::AtLeast, 0.5, 0.1);
        let shown = r
            .cells
            .iter()
            .filter_map(|c| match c {
                Cell::Value(s) => Some(s.clone()),
                _ => None,
            })
            .collect::<String>();
        assert!(shown.contains("0.50"), "the threshold is shown: {shown}");
        assert!(shown.contains("0.10"), "and the live reading: {shown}");
    }

    #[test]
    fn the_boundary_reads_as_met_on_both_signs() {
        // Matches `FieldCondition::is_met`, which is inclusive — the HUD must not disagree with the rule
        // it is describing, or a player at exactly the threshold sees "not met" while the capture ticks.
        assert_eq!(clause_row("X", Sign::AtLeast, 0.5, 0.5).emphasis, Emphasis::Muted);
        assert_eq!(clause_row("X", Sign::AtMost, 0.5, 0.5).emphasis, Emphasis::Muted);
        // …and the bar agrees with the verdict, rather than reading 99% at the exact boundary.
        assert_eq!(clause_progress(Sign::AtLeast, 0.5, 0.5), 1.0);
        assert_eq!(clause_progress(Sign::AtMost, 0.5, 0.5), 1.0);
    }

    #[test]
    fn the_clause_bar_moves_the_right_way_for_each_sign() {
        // Getting this backwards would draw a FULL bar for the worst possible reading, which is a
        // lie the player would act on.
        assert!(clause_progress(Sign::AtLeast, 0.5, 0.1) < clause_progress(Sign::AtLeast, 0.5, 0.4));
        assert!(clause_progress(Sign::AtMost, 0.1, 0.9) < clause_progress(Sign::AtMost, 0.1, 0.2));
        // Degenerate thresholds must not divide by zero or escape [0,1].
        for p in [
            clause_progress(Sign::AtLeast, 0.0, 0.0),
            clause_progress(Sign::AtMost, 1.0, 5.0),
            clause_progress(Sign::AtLeast, 0.5, -3.0),
        ] {
            assert!((0.0..=1.0).contains(&p), "progress escaped its range: {p}");
        }
    }

    #[test]
    fn exactly_the_unmet_clauses_are_loud() {
        // The panel's whole job: the player's eye must land on what to DO. If every row were Alert
        // (or none were), the emphasis would carry no information at all.
        let rows = readout_rows(
            0.4,
            1.7,
            4.0,
            &[
                ("GUNFIRE", Sign::AtMost, 0.05, 0.01),
                ("OBSERVATION", Sign::AtLeast, 0.50, 0.10),
            ],
            true,
        );
        let loud: Vec<_> = rows.iter().filter(|r| r.emphasis == Emphasis::Alert).collect();
        assert_eq!(loud.len(), 1, "exactly the one unmet clause should be loud");
        assert!(label_of(loud[0]).contains("OBSERVATION"));
    }

    #[test]
    fn an_illegible_procedure_states_the_route_rather_than_going_blank() {
        // An empty panel reads as a bug. The player must learn the information is OBTAINABLE.
        let rows = readout_rows(0.1, 0.0, 4.0, &[("GUNFIRE", Sign::AtMost, 0.05, 0.9)], false);
        let text: String = rows.iter().filter_map(|r| r.label()).collect::<Vec<_>>().join(" ");
        assert!(text.contains("PROCEDURE UNKNOWN"), "{text}");
        assert!(!text.contains("GUNFIRE"), "an unstudied anomaly must not leak its clauses: {text}");
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
