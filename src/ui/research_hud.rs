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
use bevy::ui_widgets::ScrollArea;

use super::layout::{self, HudRegions, Region};
use super::rows::{sync_rows, Cell, Emphasis, Row, RowPanel};
use super::state::{despawn_scoped, AppState};
use super::theme::{glyph, FontAssets, UiTheme};
use super::widgets::{border_all, text_colored};
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
        app.add_systems(OnEnter(AppState::Site), spawn_panel.after(layout::spawn_frame))
            .add_systems(OnExit(AppState::Site), despawn_scoped::<ResearchHudRoot>)
            .add_systems(
                Update,
                // NO `RunState::Active` gate here, and that is load-bearing: research happens at the
                // SITE, which is only ever entered by `RETURN TO SITE` — and that sets `RunState::Idle`
                // in the same transition. `Site ∧ Active` is never true, so gating on it silently kills
                // the research verb (all of FVS-E-5). It was added by FVS-G-6's sweep, which matched
                // registrations by system NAME: `ui::containment_hud` has a *different* `update_readout`
                // that does take `Res<Dungeon>`, and the bare name collided. Neither system here takes a
                // run-scoped resource, so there is nothing to gate.
                (request_experiment, update_readout, update_run_button)
                    .run_if(in_state(AppState::Site)),
            );
    }
}

fn spawn_panel(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    regions: Res<HudRegions>,
) {
    let root = (
        ResearchHudRoot,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_sm)),
            min_width: Val::Px(320.0),
            max_height: Val::Percent(100.0),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        BackgroundColor(theme.panel),
        border_all(theme.panel_border),
        ScrollArea,
    );
    let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::TopRight, root) else {
        error!("research HUD: no layout frame at spawn — the stat sheet is not shown");
        return;
    };
    ec.with_children(|p| {
        p.spawn((
            ResearchReadout,
            RowPanel::default(),
            Node { flex_direction: FlexDirection::Column, ..default() },
            Pickable::IGNORE,
        ));
        p.spawn((
            RunTestButton,
            bevy::ui_widgets::Button,
            bevy::picking::hover::Hovered::default(),
            Node {
                padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_xs)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(theme.radius)),
                margin: UiRect::top(Val::Px(theme.space_sm)),
                ..default()
            },
            BackgroundColor(theme.panel),
            border_all(theme.panel_border),
        ))
        .observe(
            |_: On<bevy::ui_widgets::Activate>,
             mut out: MessageWriter<crate::research::RunExperiment>| {
                // The SAME intent message the key sends; `research::lab::run_experiments` is the
                // single writer either way, so the click and the key cannot diverge.
                out.write(crate::research::RunExperiment);
            },
        )
        .with_children(|b| {
            b.spawn((
                RunTestLabel,
                text_colored(&theme, &fonts, "", theme.font_body, theme.text),
                Pickable::IGNORE,
            ));
        });
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
fn param_row(p: HiddenParam, belief: f32, revealed: bool) -> Row {
    let name = param_name(p);
    if revealed {
        let verdict = if belief >= 0.5 { "CONFIRMED" } else { "RULED OUT" };
        Row::kv(name, verdict)
            .with_glyph(glyph::DONE)
            .with_emphasis(Emphasis::Muted)
    } else {
        // The bar is the belief, so "how unsure am I" is a LENGTH rather than a number the player has
        // to parse — and it keeps the fog visible at a glance next to the resolved rows.
        Row::kv(name, format!("UNRESOLVED ({:.0}%)", belief * 100.0))
            .with_glyph(glyph::LOCKED)
            .push(Cell::Bar { frac: belief })
    }
}

/// How many experiments are **actually on offer**: strictly positive expected information gain, capped
/// at what the panel will render.
///
/// Shared by the panel and the RUN button so the two cannot disagree. They did: the button counted
/// `rank_by_information_gain(..).len()` — every experiment in the battery, including the ones worth
/// zero bits — so it read `RUN THE TOP TEST` while the panel right above it read
/// `NO INFORMATIVE TEST REMAINS`. Two implementations of one question, and the one the player acts on
/// was the wrong one.
pub fn live_offers(experiments: &[Experiment], posterior: &ResearchPosterior) -> usize {
    rank_by_information_gain(experiments, posterior)
        .iter()
        .take(OFFERED)
        .filter(|&&i| {
            experiments
                .get(i)
                .is_some_and(|e| e.expected_information_gain(posterior) > 0.0)
        })
        .count()
}

/// One offered experiment, with the reason it is offered.
///
/// Bits, not a percentage: the quantity really is information, and rounding it to a percentage of
/// nothing-in-particular would be a number that looks meaningful and is not. It is rendered as a
/// [`Cell::Delta`] — an explicitly signed CHANGE — because that is the quantity the player responds
/// to. Andersen, Miller, Kiverstein & Deterding 2022 (DOI 10.3389/fpsyg.2022.924953) argue players
/// are "sensitive not just to absolute error, but also to changes in the rate of error reduction",
/// so a panel that shows only the standing posterior withholds the part that carries the affect.
fn experiment_row(rank: usize, name: &str, bits: f32, top: bool) -> Row {
    Row::kv(format!("{}. {name}", rank + 1), "")
        .with_glyph(if top { glyph::CURRENT } else { "" })
        .with_emphasis(if top { Emphasis::Alert } else { Emphasis::Normal })
        .push(Cell::Delta(bits))
}

/// Player-facing name of a specimen's species. A test pins that every one is named, so a new
/// `Subject` cannot reach the panel as a debug string.
pub fn subject_name(s: crate::knowledge::Subject) -> &'static str {
    use crate::knowledge::Subject;
    match s {
        Subject::Crabs => "DIMENSIONAL CRABS",
        Subject::Flesh => "SCP-610",
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
) -> Vec<Row> {
    let mut out = vec![Row::header(format!("RESEARCH — {}", subject_name(subject)))];
    for p in HiddenParam::ALL {
        out.push(param_row(p, posterior.belief(p), posterior.is_revealed(p)));
    }
    // The prerequisite gate, stated as an instruction rather than a refusal — the same rule FVS-L-1
    // set for containment clauses ("RAISE OBSERVATION", not "unmet"). A bare "unavailable" leaves the
    // player with nothing to do about it.
    if !unmet.is_empty() {
        out.push(Row::header("AWAITING PRIOR RESEARCH"));
        for c in unmet {
            out.push(Row::note(c.label()).with_indent(1).with_glyph(glyph::LOCKED));
        }
        return out;
    }
    if finished {
        // The payout is the point of the arc, so say so rather than leaving an empty list that reads
        // like a bug.
        out.push(Row::kv("RESEARCH COMPLETE", "CAPABILITY DERIVED").with_glyph(glyph::DONE));
        return out;
    }

    // The standing level of uncertainty…
    let entropy = posterior.total_entropy();
    out.push(Row::kv("REMAINING UNCERTAINTY", format!("{entropy:.2} bits")));

    let ranked = rank_by_information_gain(experiments, posterior);
    let mut offered = 0;
    let mut rows = Vec::new();
    for &i in &ranked {
        let bits = experiments[i].expected_information_gain(posterior);
        if bits <= 0.0 || offered >= OFFERED {
            break;
        }
        rows.push(experiment_row(offered, &experiments[i].name, bits, offered == 0));
        offered += 1;
    }
    if offered == 0 {
        out.push(Row::note("NO INFORMATIVE TEST REMAINS"));
        return out;
    }
    // …and, right beside it, how far the next action would MOVE it. That pairing is the point: the
    // level alone tells the player where they are, the delta tells them whether the next test is
    // worth running, which is the decision this panel exists to support.
    // The header names the SECTION; the key lives on the real button below the panel. It used to read
    // `[R] RUN THE TOP TEST` — a header styled like a control that could not be clicked, which is the
    // "row of things that look like buttons and are not" failure `ui::verb_bar`'s header names.
    out.push(Row::header("OFFERED TESTS"));
    out.extend(rows);
    out
}

/// Ask the bench to run the top-ranked test on the studied specimen.
///
/// Only *asks* — `research::lab::run_experiments` decides and is the single writer, the same discipline
/// `session::ForceVictory` and `parasite::CureRequest` use. The key itself lives in `crate::input`,
/// which is also what proves it does not collide with anything that can be live at the same time.
fn request_experiment(
    actions: crate::input::Actions,
    mut out: MessageWriter<crate::research::RunExperiment>,
) {
    if actions.just_pressed(crate::input::Action::RunTopExperiment) {
        out.write(crate::research::RunExperiment);
    }
}

/// The clickable "run the top test" button. Spawned once, outside the `RowPanel` subtree, because
/// `rows::sync_rows` despawns that subtree whenever the offers change — which running a test does.
#[derive(Component)]
pub struct RunTestButton;

/// Its label node, so the text can be rewritten without respawning the button.
#[derive(Component)]
pub struct RunTestLabel;

/// What the button reads. Pure, states its key, and distinguishes "nothing left to learn" from a dead
/// control — `docs/ui.md` §1.4's rule that an unmet condition is an instruction.
pub fn run_test_label(offers: usize, bindings: &crate::input::KeyBindings) -> String {
    // The LIVE binding, not `default_binding()`.
    let key = bindings.key_label(crate::input::Action::RunTopExperiment);
    if offers == 0 {
        format!("{key}  NO INFORMATIVE TEST REMAINS")
    } else {
        format!("{key}  RUN THE TOP TEST")
    }
}

/// Keep the run button's label and ink in step with whether a test is on offer.
fn update_run_button(
    theme: Res<UiTheme>,
    bindings: Res<crate::input::KeyBindings>,
    specimens: Query<(&crate::containment::Specimen, &ResearchPosterior, Option<&Researched>)>,
    studied: Res<crate::research::StudySubject>,
    curriculum: Res<crate::research::Curriculum>,
    mut labels: Query<(&mut Text, &mut TextColor), With<RunTestLabel>>,
    mut buttons: Query<
        (&bevy::picking::hover::Hovered, &mut BackgroundColor, &mut BorderColor),
        With<RunTestButton>,
    >,
) {
    // "Is a test on offer" is derived the SAME way the panel derives it — through
    // `rank_by_information_gain` on the studied specimen — so the button cannot promise a test the
    // panel does not list, or refuse one it does.
    // **The STUDIED specimen, resolved the same way `update_readout` resolves it.** This used to take
    // `studied.0` as a mere presence check and then pick whichever un-researched specimen the ECS
    // query happened to yield first, count `rank_by_information_gain(..).len()` rather than the offers
    // with `bits > 0`, and never consult prerequisites. With two specimens held, the panel could read
    // `NO INFORMATIVE TEST REMAINS` while this button read `R  RUN THE TOP TEST` and looked live —
    // the exact thing its doc claims to prevent. The pick was also query-order dependent, so the label
    // could differ between sessions with identical saves.
    let offers = studied
        .0
        .and_then(|e| specimens.get(e).ok())
        .filter(|(_, _, done)| done.is_none())
        .map(|(spec, post, _)| live_offers(&curriculum.experiments(spec.subject), post))
        .unwrap_or(0);
    let want = run_test_label(offers, &bindings);
    for (mut text, mut color) in &mut labels {
        if text.0 != want {
            text.0 = want.clone();
        }
        let ink = if offers > 0 { theme.text } else { theme.text_muted };
        if color.0 != ink {
            color.0 = ink;
        }
    }
    for (hovered, mut bg, mut border) in &mut buttons {
        let want_bg = if hovered.0 && offers > 0 {
            theme.panel_border.with_alpha(0.16)
        } else {
            theme.panel
        };
        let want_border =
            if offers > 0 { theme.panel_border } else { theme.panel_border.with_alpha(0.25) };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want = border_all(want_border);
        if border.top != want.top {
            *border = want;
        }
    }
}

fn update_readout(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    specimens: Query<(&crate::containment::Specimen, &ResearchPosterior, Option<&Researched>)>,
    studied: Res<crate::research::StudySubject>,
    curriculum: Res<crate::research::Curriculum>,
    tree: Res<crate::research::TechTree>,
    mut readout_q: Query<(Entity, &mut RowPanel), With<ResearchReadout>>,
) {
    // The experiment battery is read from the authored curriculum, keyed on what the specimen actually
    // IS. It used to come from an `AuthoredExperiments` resource that nothing ever inserted, so this
    // panel rendered an empty node for a whole session while its unit tests stayed green — the reason
    // FVS-E-5 exists. One source of truth now: the `research:` config slice.
    let Ok((entity, mut panel)) = readout_q.single_mut() else { return };
    let rows = match studied.0.and_then(|e| specimens.get(e).ok()) {
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
        None => vec![Row::header("RESEARCH — NO SPECIMENS HELD")],
    };
    sync_rows(&mut commands, entity, &mut panel, &theme, &fonts, rows);
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

    /// Flatten rows to readable text, for assertions about *content*. Structure (emphasis, glyph,
    /// which cell carries the delta) is asserted directly on the rows.
    fn flat(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| {
                let cells: Vec<String> = r
                    .cells
                    .iter()
                    .map(|c| match c {
                        Cell::Label(s) | Cell::Value(s) => s.clone(),
                        Cell::Delta(d) => super::super::rows::format_delta(*d),
                        Cell::Bar { frac } => format!("[bar {frac:.2}]"),
                    })
                    .collect();
                format!("{} {}", r.glyph, cells.join("  "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The panel for a specimen with no unmet prerequisites — the case every wording test below is
    /// about. The gated case has its own test rather than an argument threaded through all of them.
    fn ungated(p: &ResearchPosterior, exps: &[Experiment], finished: bool) -> Vec<Row> {
        readout(crate::knowledge::Subject::ComfortBlob, p, exps, finished, &[])
    }

    #[test]
    fn the_run_button_states_its_key_and_when_there_is_nothing_left() {
        // The `[R] RUN THE TOP TEST` *header* was a control-looking row that could not be clicked.
        // Now the header names the section and this button carries the verb — so it must state the key
        // (operability, `docs/ui.md` §4.2) and name the exhausted state rather than going dead.
        let b = crate::input::KeyBindings::default();
        let none = run_test_label(0, &b);
        let some = run_test_label(2, &b);
        for l in [&none, &some] {
            assert!(!l.trim().is_empty());
            assert!(l.starts_with('R'), "{l} must lead with its key");
        }
        assert!(none.contains("NO INFORMATIVE TEST REMAINS"), "{none}");
        assert_ne!(none, some);
    }

    #[test]
    fn a_gated_specimen_names_the_research_it_is_waiting_on() {
        // FVS-L-1's rule applied to the curriculum: say WHY, and say it as something the player can act
        // on. "Unavailable" with no name is a dead end.
        let rows = readout(
            crate::knowledge::Subject::Parasite,
            &ResearchPosterior::unknown(),
            &battery(),
            false,
            &[crate::research::Capability::MoraleField],
        );
        let out = flat(&rows);
        assert!(out.contains("AWAITING PRIOR RESEARCH"), "{out}");
        assert!(
            out.contains(crate::research::Capability::MoraleField.label()),
            "the gate must NAME the prerequisite: {out}"
        );
        assert!(
            !rows.iter().any(|r| r.cells.iter().any(|c| matches!(c, Cell::Delta(_)))),
            "a gated specimen must offer no tests: {out}"
        );
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
        let r = param_row(HiddenParam::Lethality, 0.68, false);
        assert!(flat(&[r.clone()]).contains("UNRESOLVED"));
        assert_eq!(r.glyph, glyph::LOCKED, "an open question is marked open");
        assert!(
            r.cells.iter().any(|c| matches!(c, Cell::Bar { .. })),
            "and its uncertainty is shown as a length, not only a number"
        );
    }

    #[test]
    fn a_resolved_parameter_states_a_verdict_in_both_directions() {
        // Certainty of absence is a finding too — a specimen proven harmless must not read as blank.
        assert!(flat(&[param_row(HiddenParam::Contagion, 0.97, true)]).contains("CONFIRMED"));
        assert!(flat(&[param_row(HiddenParam::Contagion, 0.02, true)]).contains("RULED OUT"));
        // A settled question recedes; it is no longer where the player should look.
        assert_eq!(param_row(HiddenParam::Contagion, 0.97, true).emphasis, Emphasis::Muted);
    }

    #[test]
    fn the_panel_offers_the_most_informative_test_first_and_says_why() {
        let mut p = ResearchPosterior::unknown();
        for _ in 0..4 {
            p.observe(HiddenParam::Lethality, true, 0.85);
        }
        let rows = ungated(&p, &battery(), false);
        let first = rows
            .iter()
            .find(|r| r.label().is_some_and(|l| l.starts_with("1.")))
            .expect("an offer");
        assert!(
            !first.label().unwrap_or_default().contains("Lethality"),
            "the nearly-settled question must not be offered first: {first:?}"
        );
        assert!(
            first.cells.iter().any(|c| matches!(c, Cell::Delta(_))),
            "the offer must state WHY it is offered — the expected gain: {first:?}"
        );
    }

    #[test]
    fn the_top_offer_is_the_loud_one() {
        // `[R]` runs the top test, so the row `[R]` would act on has to be the row the eye lands on.
        let rows = ungated(&ResearchPosterior::unknown(), &battery(), false);
        let offers: Vec<&Row> = rows
            .iter()
            .filter(|r| r.cells.iter().any(|c| matches!(c, Cell::Delta(_))))
            .collect();
        assert!(!offers.is_empty(), "there should be offers");
        assert_eq!(offers[0].emphasis, Emphasis::Alert, "the top offer is emphasised");
        for other in &offers[1..] {
            assert_ne!(other.emphasis, Emphasis::Alert, "only the top offer is: {other:?}");
        }
    }

    #[test]
    fn the_expected_gain_is_shown_as_a_signed_change_not_a_bare_level() {
        // Andersen et al. 2022: affect tracks the RATE of error reduction. A panel showing only the
        // standing posterior withholds the quantity the player actually responds to, so the offer
        // carries a signed delta and the standing uncertainty is a separate row.
        let rows = ungated(&ResearchPosterior::unknown(), &battery(), false);
        let out = flat(&rows);
        assert!(out.contains("REMAINING UNCERTAINTY"), "the level is still shown: {out}");
        assert!(out.contains('+'), "and the change is signed: {out}");
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
        let rows = ungated(&ResearchPosterior::unknown(), &many, false);
        let offers = rows
            .iter()
            .filter(|r| r.cells.iter().any(|c| matches!(c, Cell::Delta(_))))
            .count();
        assert!(offers <= OFFERED, "offered {offers}, cap is {OFFERED}");
    }

    #[test]
    fn a_specimen_with_no_informative_test_left_says_so() {
        // An empty list reads as a bug. Say the arc is done rather than showing nothing.
        let out = flat(&ungated(&ResearchPosterior::unknown(), &[], false));
        assert!(out.contains("NO INFORMATIVE TEST REMAINS"), "{out}");
    }

    #[test]
    fn a_finished_specimen_reads_as_finished_rather_than_as_an_empty_list() {
        let out = flat(&ungated(&ResearchPosterior::unknown(), &battery(), true));
        assert!(out.contains("RESEARCH COMPLETE"), "{out}");
    }
}
