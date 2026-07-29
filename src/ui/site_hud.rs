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
use bevy::ui_widgets::ScrollArea;

use super::layout::{self, HudRegions, Region};
use super::rows::{sync_rows, Cell, Emphasis, Row, RowPanel};
use super::state::{despawn_scoped, AppState};
use super::theme::{glyph, FontAssets, UiTheme};
use super::widgets::border_all;
use crate::containment::Specimen;
use crate::knowledge::Subject;
use crate::research::{Curriculum, Researched, ResearchPosterior, StudySubject, TechTree};

/// Root marker for the Site panel.
#[derive(Component)]
pub struct SiteHudRoot;

#[derive(Component)]
pub struct SiteHudReadout;

/// One curriculum node, as rows.
///
/// `studied` marks the specimen currently on the slab, so the selector's effect is visible in the same
/// place the choice is made — now as a `»` glyph plus an emphasis step rather than a leading `>`,
/// which was findable only by reading.
///
/// The four states (locked / not-captured / held / researched) stay **four distinct rows**. Collapsing
/// any pair would hide the actual next action, which differs for each: satisfy a prerequisite, go
/// catch one, run an experiment, or nothing.
pub fn curriculum_rows(
    subject: Subject,
    name: &str,
    payouts: &[crate::research::Capability],
    unmet: &[crate::research::Capability],
    is_goal: bool,
    held: bool,
    done: bool,
    studied: bool,
) -> Vec<Row> {
    let _ = subject;
    let title = if is_goal {
        // [PROG]'s boss level. Naming it is the difference between a ramp and a path.
        format!("{name}  (GOAL)")
    } else {
        name.to_string()
    };

    let head = if studied {
        Row::kv(title, "STUDYING")
            .with_glyph(glyph::CURRENT)
            .with_emphasis(Emphasis::Alert)
    } else if done {
        Row::kv(title, "RESEARCHED")
            .with_glyph(glyph::DONE)
            .with_emphasis(Emphasis::Muted)
    } else if !unmet.is_empty() {
        Row::kv(title, "LOCKED")
            .with_glyph(glyph::LOCKED)
            .with_emphasis(Emphasis::Muted)
    } else if held {
        Row::kv(title, "READY TO STUDY").with_glyph(glyph::MET)
    } else {
        Row::kv(title, "NOT YET CAPTURED").with_glyph(glyph::MET).with_emphasis(Emphasis::Muted)
    };

    let mut rows = vec![head];
    if !unmet.is_empty() {
        // The rule FVS-L-1 set: say what unblocks it, not merely that it is blocked.
        let names: Vec<&str> = unmet.iter().map(|c| c.label()).collect();
        rows.push(Row::note(format!("NEEDS: {}", names.join(", "))).with_indent(1));
    }
    for c in payouts {
        rows.push(Row::note(format!("\u{2192} {}", c.label())).with_indent(1));
    }
    rows
}

/// The whole panel.
pub fn site_rows(
    curriculum: &Curriculum,
    tree: &TechTree,
    held: &[(Subject, bool, bool)],
    name_of: impl Fn(Subject) -> &'static str,
) -> Vec<Row> {
    let derived = tree.count();
    let total = crate::research::Capability::ALL.len();
    let mut out = vec![
        Row::header("SITE-67 — THAUMIEL CURRICULUM"),
        Row::kv("CAPABILITIES DERIVED", format!("{derived}/{total}"))
            .push(Cell::Bar { frac: if total == 0 { 0.0 } else { derived as f32 / total as f32 } }),
        Row::note("[TAB] SELECT SPECIMEN"),
    ];
    let goals = curriculum.goals();
    for subject in curriculum.progression() {
        let entry = held.iter().find(|(s, _, _)| *s == subject);
        out.extend(curriculum_rows(
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

fn spawn_panel(mut commands: Commands, theme: Res<UiTheme>, regions: Res<HudRegions>) {
    let root = (
        SiteHudRoot,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_sm)),
            // The curriculum grows with the content pack, and an unbounded panel would run off the
            // bottom of the screen with the goal node — the one the player most needs — below the
            // fold. Cap it and scroll.
            max_height: Val::Percent(100.0),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        BackgroundColor(theme.panel),
        border_all(theme.panel_border),
        // Scrollable, so it must accept the pointer — `bevy_ui_widgets::ScrollArea` reads hover.
        ScrollArea,
    );
    let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::TopLeft, root) else {
        error!("site HUD: no layout frame at spawn — the curriculum is not shown");
        return;
    };
    ec.with_children(|p| {
        p.spawn((
            SiteHudReadout,
            RowPanel::default(),
            Node { flex_direction: FlexDirection::Column, ..default() },
            Pickable::IGNORE,
        ));
    });
}

fn update_panel(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    curriculum: Res<Curriculum>,
    tree: Res<TechTree>,
    studied: Res<StudySubject>,
    specimens: Query<(Entity, &Specimen, Option<&Researched>, &ResearchPosterior)>,
    mut readout: Query<(Entity, &mut RowPanel), With<SiteHudReadout>>,
) {
    let Ok((entity, mut panel)) = readout.single_mut() else { return };
    // SORT-OK: `held` is only ever probed by `find` on `subject` below, never iterated for output —
    // the panel's row order comes from `curriculum.progression()`, which is authored, not from this.
    let held: Vec<(Subject, bool, bool)> = specimens
        .iter()
        .map(|(e, s, done, _)| (s.subject, done.is_some(), studied.0 == Some(e)))
        .collect();
    let rows = site_rows(&curriculum, &tree, &held, super::research_hud::subject_name);
    sync_rows(&mut commands, entity, &mut panel, &theme, &fonts, rows);
}

pub struct SiteHudPlugin;

impl Plugin for SiteHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Site), spawn_panel.after(layout::spawn_frame))
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

    /// Flatten rows to the text a player would read, for assertions about *content*.
    /// Structure (emphasis, glyph) is asserted directly on the rows.
    fn flat(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| {
                let cells: Vec<String> = r
                    .cells
                    .iter()
                    .filter_map(|c| match c {
                        Cell::Label(s) | Cell::Value(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
                format!("{} {}", r.glyph, cells.join("  "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_locked_node_names_the_research_that_unlocks_it() {
        // FVS-L-1's rule applied to the curriculum: a bare padlock leaves the player with nothing to
        // do about it.
        let rows = site_rows(&chain(), &TechTree::default(), &[], name_of);
        let out = flat(&rows);
        assert!(out.contains("LOCKED"), "{out}");
        assert!(
            out.contains(Capability::MoraleField.label()),
            "the lock must name its prerequisite: {out}"
        );
        // And the locked node carries the locked glyph, so the state survives without colour.
        assert!(
            rows.iter().any(|r| r.glyph == glyph::LOCKED),
            "a locked node needs its own glyph, not just the word"
        );
    }

    #[test]
    fn the_list_reads_as_a_path_prerequisites_first_and_the_goal_marked() {
        // The whole reason the order is derived rather than authored ([PROG]): a prerequisite must
        // never be listed after the thing that needs it, and the progression needs something to build
        // toward.
        let out = flat(&site_rows(&chain(), &TechTree::default(), &[], name_of));
        let blob = out.find("SCP-999").expect("999 listed");
        let para = out.find("SCP-150").expect("150 listed");
        assert!(blob < para, "the prerequisite must be listed first:\n{out}");
        assert!(out.contains("(GOAL)"), "the curriculum must name what it builds toward: {out}");
    }

    #[test]
    fn unlocking_the_prerequisite_opens_the_next_node() {
        let mut tree = TechTree::default();
        tree.grant(Capability::MoraleField);
        let rows = site_rows(&chain(), &tree, &[], name_of);
        assert!(!flat(&rows).contains("LOCKED"), "nothing should still be locked");
        assert!(
            !rows.iter().any(|r| r.glyph == glyph::LOCKED),
            "and no node should still carry the locked glyph"
        );
    }

    #[test]
    fn held_researched_and_uncaptured_are_three_distinct_states() {
        // Collapsing them would hide the actual next action. "Not yet captured" means go and catch
        // one; "held" means go and study it; "researched" means it already paid out.
        let mut tree = TechTree::default();
        tree.grant(Capability::MoraleField);
        let out = flat(&site_rows(
            &chain(),
            &tree,
            &[(Subject::ComfortBlob, true, false), (Subject::Parasite, false, true)],
            name_of,
        ));
        assert!(out.contains("RESEARCHED"), "{out}");

        let none_held = flat(&site_rows(&chain(), &tree, &[], name_of));
        assert!(none_held.contains("NOT YET CAPTURED"), "{none_held}");

        let held_only = flat(&site_rows(
            &chain(),
            &tree,
            &[(Subject::ComfortBlob, false, false)],
            name_of,
        ));
        assert!(held_only.contains("READY TO STUDY"), "{held_only}");
    }

    #[test]
    fn the_studied_specimen_is_marked_where_the_choice_is_made() {
        let rows = site_rows(
            &chain(),
            &TechTree::default(),
            &[(Subject::ComfortBlob, false, true)],
            name_of,
        );
        let studied: Vec<&Row> = rows.iter().filter(|r| r.glyph == glyph::CURRENT).collect();
        assert_eq!(studied.len(), 1, "exactly one row is the current selection");
        assert!(
            studied[0].label().unwrap_or_default().contains("SCP-999"),
            "the selector's effect must be visible in the list it selects from: {studied:?}"
        );
        // It is also the loud row — the selection is what the player just acted on.
        assert_eq!(studied[0].emphasis, Emphasis::Alert);
    }
}
