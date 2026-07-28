//! **The roster: what each operative believes** (FVS-L-5), and the cross-run carry (FVS-G-3).
//!
//! # Why a resource rather than persistent operative entities
//!
//! FVS-G-3 decided that operatives **persist across runs, carrying their knowledge**. Operatives are
//! `session::run_scoped()` entities, though — they are despawned and rebuilt every expedition — so
//! "persist" has to mean *the beliefs* persist, not the `Entity`. [`SquadKnowledge`] is that: a small
//! meta-progress resource keyed by [`crate::squad::SquadMember`] index, mirrored **from** live
//! operatives during an expedition and restored **onto** them when the next one is built.
//!
//! Making the entities themselves immortal would have been the other option, and it is worse: it
//! fights `RunState`'s teardown, which FVS-A-5 established as the single mechanism that makes
//! `NEW RUN` a genuinely fresh world.
//!
//! # Death loses what they knew, structurally
//!
//! [`sync_squad_knowledge`] **rebuilds** the table from the living rather than updating it in place, so
//! an operative who died this expedition simply has no row to contribute and their slot resets. That is
//! G-3's "a dead operative's unwritten knowledge is gone" — obtained from the shape of the sync rather
//! than from a death handler that could be forgotten. It is also what makes FVS-O-4's reports mean
//! something: a filed report is the only way knowledge survives its holder.
//!
//! # Determinism
//!
//! Windowed-only. The sync reads pinned state and writes a resource nothing pinned reads back; the
//! restore writes `Knowledge`, which *is* pinned — but only at `RunBuild::PostPopulate`, before the
//! first fixed tick of the run, exactly like every other world-construction step.

use bevy::prelude::*;

use super::{Claim, Knowledge, Subject};
use crate::squad::{SquadMember, Unit};
use crate::ui::state::{despawn_scoped, AppState, MenuState};

/// How many operatives the table holds. `spawn_squad` builds five; the extra headroom costs nothing and
/// means a larger squad does not silently truncate.
pub const ROSTER_SLOTS: usize = 8;

/// What the squad knows, as meta-progress. Not run-scoped.
#[derive(Resource, Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SquadKnowledge {
    /// Indexed by `SquadMember.0`.
    pub members: [Knowledge; ROSTER_SLOTS],
}

impl Default for SquadKnowledge {
    fn default() -> Self {
        Self { members: [Knowledge::default(); ROSTER_SLOTS] }
    }
}

/// Mirror live operatives into the table, **rebuilding** it so the dead drop out.
pub fn sync_squad_knowledge(
    mut table: ResMut<SquadKnowledge>,
    operatives: Query<(&SquadMember, &Knowledge), With<Unit>>,
) {
    let mut next = [Knowledge::default(); ROSTER_SLOTS];
    for (member, knowledge) in &operatives {
        // A member index beyond the table is dropped rather than panicking — `ROSTER_SLOTS` is
        // headroom, and a larger squad is a content change, not a crash.
        if let Some(slot) = next.get_mut(member.0) {
            *slot = *knowledge;
        }
    }
    // Order-independent by construction: each operative writes only its own slot, and the slot index
    // is `SquadMember`, the stable identity every other site keys on. No sort needed.
    if table.members != next {
        table.members = next;
    }
}

/// Hand last expedition's beliefs to the operatives just spawned for this one.
pub fn restore_squad_knowledge(
    table: Res<SquadKnowledge>,
    mut operatives: Query<(&SquadMember, &mut Knowledge), With<Unit>>,
) {
    for (member, mut knowledge) in &mut operatives {
        if let Some(saved) = table.members.get(member.0) {
            *knowledge = *saved;
        }
    }
}

/// Root marker for the roster overlay.
#[derive(Component)]
pub struct RosterScreenRoot;

#[derive(Component)]
pub struct RosterReadout;

/// Player-facing name of a claim.
fn claim_word(c: Claim) -> &'static str {
    match c {
        Claim::Lethal => "LETHAL",
        Claim::Harmless => "HARMLESS",
        Claim::Containable => "CONTAINABLE",
    }
}

/// One operative's page.
///
/// **Provenance and confidence are both printed**, and that is the requirement rather than decoration:
/// FVS-O-5's whole counter-play is the player noticing that a belief is *hearsay* and going to verify
/// it firsthand. A line that said only "Okafor thinks 1048-A is lethal" would hide the one field that
/// makes a false belief actionable.
pub fn roster_line(name: &str, knowledge: &Knowledge) -> String {
    let mut out = format!("{name}\n");
    let mut any = false;
    for subject in Subject::ALL {
        for claim in Claim::ALL {
            let Some(b) = knowledge.of(subject, claim) else { continue };
            any = true;
            out.push_str(&format!(
                "  {:?} IS {} — {:?}, {:.0}%\n",
                subject,
                claim_word(claim),
                b.provenance,
                b.confidence * 100.0
            ));
        }
    }
    if !any {
        // An operative who has met nothing is a real and distinct state — "unknown" is not "unsure"
        // (the Fisher point the whole model is built on), so it gets a sentence rather than a blank.
        out.push_str("  NO FIELD EXPERIENCE ON RECORD\n");
    }
    out
}

/// The whole screen.
pub fn roster_text(table: &SquadKnowledge, names: &[String]) -> String {
    let mut out = String::from("OPERATIVE ROSTER — WHAT THEY BELIEVE\n\n");
    for (i, k) in table.members.iter().enumerate() {
        let unnamed = format!("OPERATIVE {i}");
        let name = names.get(i).unwrap_or(&unnamed);
        // Only show slots that are actually staffed: trailing headroom would read as phantom operatives.
        if i >= names.len().max(1) && !k.knows_anything() {
            continue;
        }
        out.push_str(&roster_line(name, k));
        out.push('\n');
    }
    out
}

impl Knowledge {
    /// Does this operative believe anything at all about anything?
    pub fn knows_anything(&self) -> bool {
        Subject::ALL.iter().any(|s| self.knows(*s))
    }
}

fn toggle_roster(
    keys: Res<ButtonInput<KeyCode>>,
    current: Res<State<MenuState>>,
    mut next: ResMut<NextState<MenuState>>,
) {
    // `L` for roster — free, and mnemonic for the list. `set_if_neq` is not needed because the target
    // always differs from the source of the branch.
    if keys.just_pressed(KeyCode::KeyL) {
        match current.get() {
            MenuState::Closed => next.set(MenuState::Roster),
            MenuState::Roster => next.set(MenuState::Closed),
            _ => {}
        }
    }
}

fn spawn_roster(
    mut commands: Commands,
    theme: Res<crate::ui::theme::UiTheme>,
    fonts: Res<crate::ui::theme::FontAssets>,
    table: Res<SquadKnowledge>,
) {
    let text = roster_text(&table, &[]);
    commands
        .spawn((
            RosterScreenRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(theme.space_lg),
                left: Val::Px(theme.space_lg),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            GlobalZIndex(crate::ui::theme::Z_MENU),
        ))
        .with_children(|p| {
            p.spawn((
                RosterReadout,
                crate::ui::widgets::text_colored(&theme, &fonts, text, theme.font_body, theme.text),
            ));
        });
}

fn update_roster(
    table: Res<SquadKnowledge>,
    mut text_q: Query<&mut Text, With<RosterReadout>>,
) {
    if !table.is_changed() {
        return;
    }
    let line = roster_text(&table, &[]);
    for mut t in &mut text_q {
        if t.0 != line {
            t.0 = line.clone();
        }
    }
}

/// FVS-L-5 plus the cross-run carry. **Windowed-only** except the restore, which is world construction.
pub struct RosterPlugin;

impl Plugin for RosterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SquadKnowledge>()
            // The research briefing (FVS-O-2's benefit half) lives HERE rather than in
            // `ResearchPlugin`, because it writes the resource this plugin owns. `ResearchPlugin` is
            // harness-visible; this one is not, and a system that writes windowed-only meta-progress
            // must be registered where that resource is guaranteed to exist.
            .add_systems(
                Update,
                crate::research::unlock::brief_the_squad_on_completed_research
                    .after(crate::research::unlock::finish_completed_research),
            )
            .add_systems(
                OnEnter(crate::session::RunState::Active),
                restore_squad_knowledge.in_set(crate::session::RunBuild::PostPopulate),
            )
            .add_systems(
                Update,
                sync_squad_knowledge.run_if(in_state(crate::session::RunState::Active)),
            )
            .add_systems(OnEnter(MenuState::Roster), spawn_roster)
            .add_systems(OnExit(MenuState::Roster), despawn_scoped::<RosterScreenRoot>)
            .add_systems(Update, update_roster.run_if(in_state(MenuState::Roster)))
            .add_systems(
                Update,
                // `InGame` ONLY, and the `.or_else(in_state(AppState::Site))` that used to be here was
                // not a feature — it was a crash. `MenuState` is a SubState sourced on
                // `AppState::InGame`, so Bevy REMOVES `State<MenuState>` the moment the app leaves it;
                // this system takes `Res<State<MenuState>>` non-optionally and panicked on the first
                // frame at the Site with "Parameter `Res<State<MenuState>>` failed validation: Resource
                // does not exist". Reported from real play 2026-07-28, reproduced by
                // `replay::returning_to_the_site_after_a_run_does_not_panic`.
                //
                // Restricting rather than wrapping in `Option`, because the roster could never have
                // opened at the Site regardless: `spawn_roster` hangs off `OnEnter(MenuState::Roster)`,
                // and that state does not exist there either. An `Option` would silence the panic and
                // leave a key that does nothing — a worse failure, because it looks supported.
                // Reviewing the roster between expeditions is a real want; it needs a Site-side screen
                // of its own, the way `ui::site_hud` works. Tracked as FVS-L-6.
                toggle_roster.run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::Provenance;

    #[test]
    fn the_roster_states_provenance_and_confidence_not_just_the_claim() {
        // FVS-O-5 depends on the player being able to see that a belief is HEARSAY. A line that only
        // said "believes 1048-A is lethal" would hide the field that makes a false belief actionable.
        let mut k = Knowledge::default();
        k.learn(Subject::BearCopies, Claim::Lethal, Provenance::Told, 3);
        let line = roster_line("OKAFOR", &k);
        assert!(line.contains("LETHAL"), "{line}");
        assert!(line.contains("Told"), "provenance must be visible: {line}");
        assert!(line.contains('%'), "confidence must be visible: {line}");
    }

    #[test]
    fn an_operative_who_has_met_nothing_says_so_rather_than_rendering_blank() {
        // "Never encountered" is a real state, distinct from "unsure" — the Fisher point the whole
        // model rests on. A blank page would read as a bug.
        let line = roster_line("NDIAYE", &Knowledge::default());
        assert!(line.contains("NO FIELD EXPERIENCE"), "{line}");
    }

    #[test]
    fn a_dead_operatives_knowledge_is_lost_when_the_table_resyncs() {
        // G-3's stake, and it is structural: the sync REBUILDS from the living, so a member with no
        // row contributes nothing and their slot resets. That is what makes FVS-O-4's reports matter.
        let mut app = App::new();
        app.init_resource::<SquadKnowledge>().add_systems(Update, sync_squad_knowledge);

        let mut veteran = Knowledge::default();
        veteran.learn(Subject::BearCopies, Claim::Lethal, Provenance::Firsthand, 1);
        let e = app.world_mut().spawn((Unit, SquadMember(2), veteran)).id();
        app.update();
        assert!(
            app.world().resource::<SquadKnowledge>().members[2].knows(Subject::BearCopies),
            "a living veteran's knowledge must reach the table"
        );

        app.world_mut().entity_mut(e).despawn();
        app.update();
        assert!(
            !app.world().resource::<SquadKnowledge>().members[2].knows(Subject::BearCopies),
            "and it must die with them — a report is the only thing that outlives an operative"
        );
    }

    #[test]
    fn beliefs_carry_into_the_next_expedition() {
        // The cross-run half of G-3: a fresh operative entity inherits the slot's knowledge, so
        // veterans diverge and squad selection becomes a real decision.
        let mut table = SquadKnowledge::default();
        table.members[0].learn(Subject::ComfortBlob, Claim::Harmless, Provenance::Firsthand, 9);

        let mut app = App::new();
        app.insert_resource(table).add_systems(Update, restore_squad_knowledge);
        let e = app.world_mut().spawn((Unit, SquadMember(0), Knowledge::default())).id();
        app.update();
        assert!(
            app.world().get::<Knowledge>(e).expect("knowledge").knows(Subject::ComfortBlob),
            "a rebuilt operative must inherit what their predecessor slot knew"
        );
    }
}
