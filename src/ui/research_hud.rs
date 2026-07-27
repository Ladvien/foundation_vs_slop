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
            .add_systems(Update, update_readout.run_if(in_state(AppState::Site)));
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

/// The whole panel.
fn readout(
    posterior: &ResearchPosterior,
    experiments: &[Experiment],
    finished: bool,
) -> String {
    let mut out = String::from("RESEARCH — SPECIMEN\n");
    for p in HiddenParam::ALL {
        out.push_str(&param_line(p, posterior.belief(p), posterior.is_revealed(p)));
        out.push('\n');
    }
    if finished {
        // The payout is the point of the arc, so say so rather than leaving an empty list that reads
        // like a bug.
        out.push_str("\nRESEARCH COMPLETE — CAPABILITY DERIVED");
        return out;
    }
    out.push_str(&format!("\nREMAINING UNCERTAINTY: {:.2} bits\n", posterior.total_entropy()));
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

fn update_readout(
    specimens: Query<(&ResearchPosterior, Option<&Researched>)>,
    experiments: Option<Res<AuthoredExperiments>>,
    mut text_q: Query<&mut Text, With<ResearchReadout>>,
) {
    let Some(experiments) = experiments else { return };
    // The specimen under study. One at a time until FVS-L-3's Site screen offers a selector; picking
    // "the least-researched" is a deterministic choice and the useful default — it is the one with work
    // left to do.
    let mut best: Option<(&ResearchPosterior, bool)> = None;
    let mut most = -1.0f32;
    for (p, done) in &specimens {
        let e = p.total_entropy();
        if e > most {
            most = e;
            best = Some((p, done.is_some()));
        }
    }
    let line = match best {
        Some((p, done)) => readout(p, &experiments.0, done),
        None => "RESEARCH — NO SPECIMENS HELD".into(),
    };
    for mut t in &mut text_q {
        if t.0 != line {
            t.0 = line.clone();
        }
    }
}

/// The authored experiment battery.
#[derive(Resource, Debug, Clone, Default)]
pub struct AuthoredExperiments(pub Vec<Experiment>);

#[cfg(test)]
mod tests {
    use super::*;

    fn battery() -> Vec<Experiment> {
        HiddenParam::ALL
            .iter()
            .map(|p| Experiment { name: format!("{p:?} ASSAY"), param: *p, reliability: 0.8 })
            .collect()
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
        let out = readout(&p, &battery(), false);
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
        let out = readout(&ResearchPosterior::unknown(), &many, false);
        let offers = out.lines().filter(|l| l.contains("bits)")).count();
        assert_eq!(offers, OFFERED, "an unbounded list buries the ranking's whole point");
    }

    #[test]
    fn a_finished_specimen_reads_as_finished_rather_than_as_an_empty_list() {
        let mut p = ResearchPosterior::unknown();
        for q in HiddenParam::ALL {
            p.reveal(q);
        }
        let out = readout(&p, &battery(), true);
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
        let out = readout(&p, &battery(), false);
        assert!(out.contains("NO INFORMATIVE TEST REMAINS"), "{out}");
    }
}
