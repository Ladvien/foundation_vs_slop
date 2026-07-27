//! **The research readout** (FVS-L-2) — what we know, and what to test next.
//!
//! Two questions, and the panel answers them separately because they are separate:
//!
//! * **What does the Foundation know about this specimen?** One line per hidden parameter, with the
//!   belief and whether it has been written up as fact.
//! * **What should we run next?** The experiment list, ordered by expected information gain, with the
//!   *reason* attached — "we know least about this" is the whole justification and it is short enough
//!   to print.
//!
//! Printing the reason is not decoration. FVS-L-1 established the pattern for containment: an unmet
//! clause reads as an instruction (`RAISE OBSERVATION`) rather than a status, because the acceptance
//! was "players can read *why*". Same bar here — a bare ranked list is a black box that happens to be
//! sorted, and a player cannot tell a good ordering from a broken one.
//!
//! Windowed-only. `Update` and `OnEnter`/`OnExit` only; reads state, never writes it.

use bevy::prelude::*;

use super::state::{despawn_scoped, AppState};
use super::theme::{FontAssets, UiTheme, Z_MENU};
use super::widgets::text_colored;
use crate::research::{
    rank_by_information_gain, Experiment, HiddenParam, ResearchPosterior, Researched,
};

/// Root marker for the panel.
#[derive(Component)]
pub struct ResearchHudRoot;

/// The one text node the panel rewrites.
#[derive(Component)]
pub struct ResearchReadout;

/// How many ranked experiments to offer at once.
///
/// Bounded because an unbounded list buries the top suggestion, which is the one the ranking exists to
/// surface. Three is enough to show that a choice exists without making it a spreadsheet.
const OFFERED: usize = 3;

pub struct ResearchHudPlugin;

impl Plugin for ResearchHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Site), spawn_panel)
            .add_systems(OnExit(AppState::Site), despawn_scoped::<ResearchHudRoot>)
            .add_systems(
                Update,
                (request_experiment, update_readout).run_if(in_state(AppState::Site)),
            );
    }
}

fn spawn_panel(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    commands
        .spawn((
            ResearchHudRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(theme.space_lg),
                right: Val::Px(theme.space_lg),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                ..default()
            },
            GlobalZIndex(Z_MENU - 1),
        ))
        .with_children(|p| {
            p.spawn((ResearchReadout, text_colored(&theme, &fonts, "", theme.font_body, theme.text)));
        });
}

/// Player-facing name for a hidden parameter. A `match`, so adding one fails to compile here until
/// someone decides what the player is told — the same discipline `containment_hud::channel_name` uses.
fn param_name(p: HiddenParam) -> &'static str {
    match p {
        HiddenParam::Lethality => "LETHALITY",
        HiddenParam::Contagion => "CONTAGION",
        HiddenParam::CaptureBasin => "CAPTURE BASIN",
        HiddenParam::Proliferation => "PROLIFERATION",
    }
}

/// One line of the stat sheet.
///
/// A revealed parameter states the **finding**; an unrevealed one states the **uncertainty**. Those are
/// different sentences on purpose: "68% LETHAL" invites the player to treat a guess as a fact, whereas
/// naming it as unresolved keeps the fog honest — which is the point of a fog-of-war stat sheet.
fn param_line(p: HiddenParam, belief: f32, revealed: bool) -> String {
    let name = param_name(p);
    if revealed {
        let verdict = if belief >= 0.5 { "CONFIRMED" } else { "RULED OUT" };
        format!("[*] {name}: {verdict}")
    } else {
        format!("[ ] {name}: UNRESOLVED ({:.0}%)", belief * 100.0)
    }
}

/// One offered experiment, with the reason it is offered.
fn experiment_line(rank: usize, name: &str, bits: f32) -> String {
    // Bits, not a percentage: the quantity really is information, and rounding it to a percentage of
    // nothing-in-particular would be a number that looks meaningful and is not.
    format!("{}. {name}  (+{bits:.2} bits)", rank + 1)
}

/// Player-facing name of a specimen's species. A test pins that every one is named, so a new
/// `Subject` cannot reach the panel as a debug string.
fn subject_name(s: crate::knowledge::Subject) -> &'static str {
    use crate::knowledge::Subject;
    match s {
        Subject::Crabs => "DIMENSIONAL CRABS",
        Subject::Parasite => "SCP-150",
        Subject::ComfortBlob => "SCP-999",
        Subject::BuilderBear => "SCP-1048",
        Subject::BearCopies => "SCP-1048-A",
        Subject::Watcher => "THE WATCHER",
    }
}

/// The whole panel.
fn readout(
    subject: crate::knowledge::Subject,
    posterior: &ResearchPosterior,
    experiments: &[Experiment],
    finished: bool,
    unmet: &[crate::research::Capability],
) -> String {
    let mut out = format!("RESEARCH — {}\n", subject_name(subject));
    for p in HiddenParam::ALL {
        out.push_str(&param_line(p, posterior.belief(p), posterior.is_revealed(p)));
        out.push('\n');
    }
    // The prerequisite gate, stated as an instruction rather than a refusal — the same rule FVS-L-1
    // set for containment clauses ("RAISE OBSERVATION", not "unmet"). A bare "unavailable" leaves the
    // player with nothing to do about it.
    if let Some(first) = unmet.first() {
        out.push_str("\nAWAITING PRIOR RESEARCH:\n");
        for c in unmet {
            out.push_str(&format!("  - {}\n", c.label()));
        }
        let _ = first;
        return out;
    }
    if finished {
        // The payout is the point of the arc, so say so rather than leaving an empty list that reads
        // like a bug.
        out.push_str("\nRESEARCH COMPLETE — CAPABILITY DERIVED");
        return out;
    }
    out.push_str(&format!("\nREMAINING UNCERTAINTY: {:.2} bits\n", posterior.total_entropy()));
    out.push_str("[R] RUN THE TOP TEST\n");
    let ranked = rank_by_information_gain(experiments, posterior);
    let mut offered = 0;
    for &i in &ranked {
        let bits = experiments[i].expected_information_gain(posterior);
        if bits <= 0.0 || offered >= OFFERED {
            break;
        }
        out.push_str(&experiment_line(offered, &experiments[i].name, bits));
        out.push('\n');
        offered += 1;
    }
    if offered == 0 {
        out.push_str("NO INFORMATIVE TEST REMAINS");
    }
    out
}

/// Ask the bench to run the top-ranked test on the studied specimen.
///
/// Only *asks* — `research::lab::run_experiments` decides and is the single writer, the same discipline
/// `session::ForceVictory` and `parasite::CureRequest` use. `R` is free: the digits are the time-control
/// rungs, `Q`/`E`/`WASD` the camera, and `C`/`Z`/`X`/`F` the containment verbs.
fn request_experiment(
    keys: Res<ButtonInput<KeyCode>>,
    mut out: MessageWriter<crate::research::RunExperiment>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        out.write(crate::research::RunExperiment);
    }
}

fn update_readout(
    specimens: Query<(&crate::containment::Specimen, &ResearchPosterior, Option<&Researched>)>,
    studied: Res<crate::research::StudySubject>,
    curriculum: Res<crate::research::Curriculum>,
    tree: Res<crate::research::TechTree>,
    mut text_q: Query<&mut Text, With<ResearchReadout>>,
) {
    // The experiment battery is read from the authored curriculum, keyed on what the specimen actually
    // IS. It used to come from an `AuthoredExperiments` resource that nothing ever inserted, so this
    // panel rendered an empty node for a whole session while its unit tests stayed green — the reason
    // FVS-E-5 exists. One source of truth now: the `research:` config slice.
    let line = match studied.0.and_then(|e| specimens.get(e).ok()) {
        Some((specimen, posterior, done)) => {
            let unmet = curriculum.unmet_prerequisites(specimen.subject, &tree);
            readout(
                specimen.subject,
                posterior,
                curriculum.experiments(specimen.subject),
                done.is_some(),
                &unmet,
            )
        }
        None => "RESEARCH — NO SPECIMENS HELD".into(),
    };
    for mut t in &mut text_q {
        if t.0 != line {
            t.0 = line.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery() -> Vec<Experiment> {
        HiddenParam::ALL
            .iter()
            .map(|p| Experiment { name: format!("{p:?} ASSAY"), param: *p, reliability: 0.8 })
            .collect()
    }

    /// The panel for a specimen with no unmet prerequisites — the case every wording test below is
    /// about. The gated case has its own test rather than an argument threaded through all of them.
    fn ungated(p: &ResearchPosterior, exps: &[Experiment], finished: bool) -> String {
        readout(crate::knowledge::Subject::ComfortBlob, p, exps, finished, &[])
    }

    #[test]
    fn a_gated_specimen_names_the_research_it_is_waiting_on() {
        // FVS-L-1's rule applied to the curriculum: say WHY, and say it as something the player can act
        // on. "Unavailable" with no name is a dead end.
        let out = readout(
            crate::knowledge::Subject::Parasite,
            &ResearchPosterior::unknown(),
            &battery(),
            false,
            &[crate::research::Capability::MoraleField],
        );
        assert!(out.contains("AWAITING PRIOR RESEARCH"), "{out}");
        assert!(
            out.contains(crate::research::Capability::MoraleField.label()),
            "the gate must NAME the prerequisite: {out}"
        );
        assert!(!out.contains("bits)"), "a gated specimen must offer no tests: {out}");
    }

    #[test]
    fn every_subject_has_a_player_facing_name() {
        // The same guard `containment_hud` puts on field channels: a new `Subject` must not reach the
        // panel as a debug string.
        for s in crate::knowledge::Subject::ALL {
            let n = subject_name(s);
            assert!(!n.is_empty(), "{s:?} has no player-facing name");
            assert_eq!(n, n.to_uppercase(), "{s:?} should match the HUD's voice");
        }
    }

    #[test]
    fn an_unresolved_parameter_reads_as_uncertain_not_as_a_fact() {
        // "68% LETHAL" invites the player to act on a guess as though it were a finding. Naming it
        // UNRESOLVED keeps the fog honest, which is the point of a fog-of-war stat sheet.
        let line = param_line(HiddenParam::Lethality, 0.68, false);
        assert!(line.contains("UNRESOLVED"), "{line}");
        assert!(line.starts_with("[ ]"), "{line}");
    }

    #[test]
    fn a_resolved_parameter_states_a_verdict_in_both_directions() {
        // Certainty of absence is a finding too — a specimen proven harmless must not read as blank.
        assert!(param_line(HiddenParam::Contagion, 0.97, true).contains("CONFIRMED"));
        assert!(param_line(HiddenParam::Contagion, 0.02, true).contains("RULED OUT"));
    }

    #[test]
    fn the_panel_offers_the_most_informative_test_first_and_says_why() {
        let mut p = ResearchPosterior::unknown();
        for _ in 0..4 {
            p.observe(HiddenParam::Lethality, true, 0.85);
        }
        let out = ungated(&p, &battery(), false);
        let first = out.lines().find(|l| l.starts_with("1.")).expect("an offer");
        assert!(
            !first.contains("Lethality"),
            "the nearly-settled question must not be offered first: {first}"
        );
        assert!(first.contains("bits"), "the offer must state WHY it is offered: {first}");
    }

    #[test]
    fn the_offer_list_is_bounded_so_the_top_suggestion_is_not_buried() {
        let many: Vec<Experiment> = (0..20)
            .map(|i| Experiment {
                name: format!("t{i}"),
                param: HiddenParam::ALL[i % HiddenParam::ALL.len()],
                reliability: 0.8,
            })
            .collect();
        let out = ungated(&ResearchPosterior::unknown(), &many, false);
        let offers = out.lines().filter(|l| l.contains("bits)")).count();
        assert_eq!(offers, OFFERED, "an unbounded list buries the ranking's whole point");
    }

    #[test]
    fn a_finished_specimen_reads_as_finished_rather_than_as_an_empty_list() {
        let mut p = ResearchPosterior::unknown();
        for q in HiddenParam::ALL {
            p.reveal(q);
        }
        let out = ungated(&p, &battery(), true);
        assert!(out.contains("RESEARCH COMPLETE"), "{out}");
        assert!(!out.contains("bits)"), "a finished arc must offer nothing: {out}");
    }

    #[test]
    fn a_specimen_with_no_informative_test_left_says_so() {
        // The state between "there is work to do" and "it is finished": every remaining question is
        // resolved but the arc has not been marked complete. An empty panel would read as a bug.
        let mut p = ResearchPosterior::unknown();
        for q in HiddenParam::ALL {
            p.reveal(q);
        }
        let out = ungated(&p, &battery(), false);
        assert!(out.contains("NO INFORMATIVE TEST REMAINS"), "{out}");
    }
}
