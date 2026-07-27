//! **The Site screen: the curriculum, and what is on the slab** (FVS-L-3).
//!
//! Two things the Site could not previously show:
//!
//! * **The Thaumiel graph.** FVS-F-1 shipped the capability flags and FVS-F-3 the prerequisite graph,
//!   but nothing rendered either, so the player had no way to see that research *is* a curriculum
//!   rather than a list. A locked node here names the research that unlocks it.
//! * **A specimen selector.** `ui::research_hud` says so in a comment — it picks "the least-researched"
//!   specimen as a deterministic placeholder *"until FVS-L-3's Site screen offers a selector"*. Until
//!   now the player could not choose what to study.
//!
//! # Curriculum order, not authored order
//!
//! The list is [`Curriculum::progression`] — post-order DFS from the goals, per Wang et al. 2019
//! (`[PROG]`, DOI 10.1145/3337722.3337745). So a prerequisite is always listed above the thing that
//! needs it, and the list *reads* as a path rather than as a set. The goal is marked, because their
//! finding is that a progression wants something to build **toward**: "engagement often comes from a
//! sense of accomplishment after completing hard tasks."
//!
//! # A locked node states what unlocks it
//!
//! FVS-L-1 set this rule for containment clauses — an unmet clause reads as an instruction
//! (`RAISE OBSERVATION`) rather than a status — and FVS-L-2 followed it for experiment offers. Same bar
//! here: `LOCKED — NEEDS: DEPLOY MORALE FIELD`, never a bare padlock. A curriculum the player cannot
//! read the shape of is just a list with some entries greyed out.
//!
//! Windowed-only: `Update` and `OnEnter`/`OnExit`, reads state and writes only `StudySubject`, which
//! nothing pinned reads.

use bevy::prelude::*;

use super::state::{despawn_scoped, AppState};
use super::theme::{FontAssets, UiTheme, Z_MENU};
use super::widgets::text_colored;
use crate::containment::Specimen;
use crate::knowledge::Subject;
use crate::research::{Curriculum, Researched, ResearchPosterior, StudySubject, TechTree};

/// Root marker for the Site panel.
#[derive(Component)]
pub struct SiteHudRoot;

#[derive(Component)]
pub struct SiteHudReadout;

/// One line of the curriculum.
///
/// `studied` marks the specimen currently on the slab, so the selector's effect is visible in the same
/// place the choice is made.
pub fn curriculum_line(
    subject: Subject,
    name: &str,
    payouts: &[crate::research::Capability],
    unmet: &[crate::research::Capability],
    is_goal: bool,
    held: bool,
    done: bool,
    studied: bool,
) -> String {
    let _ = subject;
    let marker = if studied { '>' } else { ' ' };
    let mut line = format!("{marker} {name}");
    if is_goal {
        // [PROG]'s boss level. Naming it is the difference between a ramp and a path.
        line.push_str("  (GOAL)");
    }
    line.push('\n');
    for c in payouts {
        line.push_str(&format!("    -> {}\n", c.label()));
    }
    if !unmet.is_empty() {
        // The rule FVS-L-1 set: say what unblocks it, not merely that it is blocked.
        let names: Vec<&str> = unmet.iter().map(|c| c.label()).collect();
        line.push_str(&format!("    LOCKED — NEEDS: {}\n", names.join(", ")));
    } else if done {
        line.push_str("    RESEARCHED\n");
    } else if held {
        line.push_str("    HELD — READY TO STUDY\n");
    } else {
        // Available in principle, but nothing has been captured. That is a different state from locked
        // and from finished, and collapsing them would hide the actual next action: go and catch one.
        line.push_str("    NOT YET CAPTURED\n");
    }
    line
}

/// The whole panel.
pub fn site_text(
    curriculum: &Curriculum,
    tree: &TechTree,
    held: &[(Subject, bool, bool)],
    name_of: impl Fn(Subject) -> &'static str,
) -> String {
    let mut out = String::from("SITE-67 — THAUMIEL CURRICULUM\n");
    out.push_str(&format!(
        "CAPABILITIES DERIVED: {}/{}\n[TAB] SELECT SPECIMEN\n\n",
        tree.count(),
        crate::research::Capability::ALL.len()
    ));
    let goals = curriculum.goals();
    for subject in curriculum.progression() {
        let entry = held.iter().find(|(s, _, _)| *s == subject);
        out.push_str(&curriculum_line(
            subject,
            name_of(subject),
            curriculum.payouts(subject),
            &curriculum.unmet_prerequisites(subject, tree),
            goals.contains(&subject),
            entry.is_some(),
            entry.map(|(_, done, _)| *done).unwrap_or(false),
            entry.map(|(_, _, studied)| *studied).unwrap_or(false),
        ));
    }
    out
}

/// Cycle the specimen on the slab.
///
/// **Ordered by `(captured_tick, entity)`** — the same total key `Specimen` documents for containment
/// cell assignment, and for the same reason: `SiteSpecimens` is a relationship target ordered by
/// *attach* order, which is not a total order, so cycling over it raw would visit specimens in an
/// order that could differ between sessions.
pub fn cycle_study_subject(
    keys: Res<ButtonInput<KeyCode>>,
    mut studied: ResMut<StudySubject>,
    specimens: Query<(Entity, &Specimen)>,
) {
    if !keys.just_pressed(KeyCode::Tab) {
        return;
    }
    let mut ordered: Vec<(u64, Entity)> =
        specimens.iter().map(|(e, s)| (s.captured_tick, e)).collect();
    if ordered.is_empty() {
        return;
    }
    // SORT-OK: `(captured_tick, Entity)` is total — the entity id breaks a same-tick double capture,
    // and it is the *whole* remaining value here rather than a prefix of one.
    ordered.sort_unstable();
    let next = match studied.0.and_then(|cur| ordered.iter().position(|(_, e)| *e == cur)) {
        Some(i) => ordered[(i + 1) % ordered.len()].1,
        None => ordered[0].1,
    };
    studied.0 = Some(next);
}

fn spawn_panel(mut commands: Commands, theme: Res<UiTheme>, fonts: Res<FontAssets>) {
    commands
        .spawn((
            SiteHudRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(theme.space_lg),
                left: Val::Px(theme.space_lg),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                ..default()
            },
            GlobalZIndex(Z_MENU - 1),
        ))
        .with_children(|p| {
            p.spawn((
                SiteHudReadout,
                text_colored(&theme, &fonts, "", theme.font_body, theme.text),
            ));
        });
}

fn update_panel(
    curriculum: Res<Curriculum>,
    tree: Res<TechTree>,
    studied: Res<StudySubject>,
    specimens: Query<(Entity, &Specimen, Option<&Researched>, &ResearchPosterior)>,
    mut text_q: Query<&mut Text, With<SiteHudReadout>>,
) {
    let held: Vec<(Subject, bool, bool)> = specimens
        .iter()
        .map(|(e, s, done, _)| (s.subject, done.is_some(), studied.0 == Some(e)))
        .collect();
    let line = site_text(&curriculum, &tree, &held, super::research_hud::subject_name);
    for mut t in &mut text_q {
        if t.0 != line {
            t.0 = line.clone();
        }
    }
}

pub struct SiteHudPlugin;

impl Plugin for SiteHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Site), spawn_panel)
            .add_systems(OnExit(AppState::Site), despawn_scoped::<SiteHudRoot>)
            .add_systems(
                Update,
                (cycle_study_subject, update_panel).run_if(in_state(AppState::Site)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::curriculum::{HiddenTruth, ResearchConfig, SubjectResearch};
    use crate::research::{Capability, Experiment, HiddenParam};

    fn entry(s: Subject, unlocks: Vec<Capability>, requires: Vec<Capability>) -> SubjectResearch {
        SubjectResearch {
            subject: s,
            truth: HiddenTruth {
                lethality: false,
                contagion: false,
                capture_basin: true,
                proliferation: false,
            },
            experiments: HiddenParam::ALL
                .iter()
                .map(|p| Experiment { name: format!("{p:?}"), param: *p, reliability: 0.85 })
                .collect(),
            unlocks,
            requires,
        }
    }

    fn chain() -> Curriculum {
        Curriculum(ResearchConfig {
            subjects: vec![
                entry(Subject::ComfortBlob, vec![Capability::MoraleField], vec![]),
                entry(
                    Subject::Parasite,
                    vec![Capability::FieldCure],
                    vec![Capability::MoraleField],
                ),
            ],
        })
    }

    fn name_of(s: Subject) -> &'static str {
        match s {
            Subject::ComfortBlob => "SCP-999",
            Subject::Parasite => "SCP-150",
            _ => "OTHER",
        }
    }

    #[test]
    fn a_locked_node_names_the_research_that_unlocks_it() {
        // FVS-L-1's rule applied to the curriculum: a bare padlock leaves the player with nothing to
        // do about it.
        let out = site_text(&chain(), &TechTree::default(), &[], name_of);
        assert!(out.contains("LOCKED — NEEDS:"), "{out}");
        assert!(
            out.contains(Capability::MoraleField.label()),
            "the lock must name its prerequisite: {out}"
        );
    }

    #[test]
    fn the_list_reads_as_a_path_prerequisites_first_and_the_goal_marked() {
        // The whole reason the order is derived rather than authored ([PROG]): a prerequisite must
        // never be listed after the thing that needs it, and the progression needs something to build
        // toward.
        let out = site_text(&chain(), &TechTree::default(), &[], name_of);
        let blob = out.find("SCP-999").expect("999 listed");
        let para = out.find("SCP-150").expect("150 listed");
        assert!(blob < para, "the prerequisite must be listed first:\n{out}");
        assert!(out.contains("(GOAL)"), "the curriculum must name what it builds toward: {out}");
    }

    #[test]
    fn unlocking_the_prerequisite_opens_the_next_node() {
        let mut tree = TechTree::default();
        tree.grant(Capability::MoraleField);
        let out = site_text(&chain(), &tree, &[], name_of);
        assert!(!out.contains("LOCKED"), "nothing should still be locked:\n{out}");
    }

    #[test]
    fn held_researched_and_uncaptured_are_three_distinct_states() {
        // Collapsing them would hide the actual next action. "Not yet captured" means go and catch
        // one; "held" means go and study it; "researched" means it already paid out.
        let mut tree = TechTree::default();
        tree.grant(Capability::MoraleField);
        let out = site_text(
            &chain(),
            &tree,
            &[(Subject::ComfortBlob, true, false), (Subject::Parasite, false, true)],
            name_of,
        );
        assert!(out.contains("RESEARCHED"), "{out}");
        assert!(out.contains("HELD — READY TO STUDY"), "{out}");

        let none_held = site_text(&chain(), &tree, &[], name_of);
        assert!(none_held.contains("NOT YET CAPTURED"), "{none_held}");
    }

    #[test]
    fn the_studied_specimen_is_marked_where_the_choice_is_made() {
        let out = site_text(
            &chain(),
            &TechTree::default(),
            &[(Subject::ComfortBlob, false, true)],
            name_of,
        );
        assert!(
            out.lines().any(|l| l.starts_with("> SCP-999")),
            "the selector's effect must be visible in the list it selects from:\n{out}"
        );
    }
}
