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

use super::{Claim, Knowledge, Provenance, Subject};
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

/// One operative's page, as rows.
///
/// **Provenance and confidence are both printed**, and that is the requirement rather than decoration:
/// FVS-O-5's whole counter-play is the player noticing that a belief is *hearsay* and going to verify
/// it firsthand. A line that said only "Okafor thinks 1048-A is lethal" would hide the one field that
/// makes a false belief actionable.
///
/// Rows rather than a `\n`-joined `String`, and here the gain is the whole mechanic: a `Told` or `Read`
/// belief is now **visibly** the loud row, because hearsay is the thing the player is meant to act on.
/// With one `TextColor` for the entire screen, a rumour and a firsthand observation rendered
/// identically and the player had to read every line for the word `Told`.
pub fn roster_rows(name: &str, knowledge: &Knowledge) -> Vec<crate::ui::rows::Row> {
    use crate::ui::rows::{Cell, Row};
    let mut rows = vec![Row::header(name)];
    let mut any = false;
    for subject in Subject::ALL {
        for claim in Claim::ALL {
            let Some(b) = knowledge.of(subject, claim) else { continue };
            any = true;
            let label = format!("{:?} IS {}", subject, claim_word(claim));
            let value = format!("{:?}", b.provenance);
            // Hearsay is actionable; firsthand experience is settled. That is the distinction the
            // emphasis carries, and it is the reason this screen exists.
            let row = if b.provenance == Provenance::Firsthand {
                Row::met(label, value)
            } else {
                Row::unmet(label, value)
            };
            rows.push(row.with_indent(1).push(Cell::Bar { frac: b.confidence }));
        }
    }
    if !any {
        // An operative who has met nothing is a real and distinct state — "unknown" is not "unsure"
        // (the Fisher point the whole model is built on), so it gets a sentence rather than a blank.
        rows.push(Row::note("NO FIELD EXPERIENCE ON RECORD").with_indent(1));
    }
    rows
}

/// The whole screen, as rows.
pub fn roster_rows_all(table: &SquadKnowledge, names: &[String]) -> Vec<crate::ui::rows::Row> {
    use crate::ui::rows::Row;
    let mut rows = vec![Row::header("OPERATIVE ROSTER — WHAT THEY BELIEVE")];
    for (i, k) in table.members.iter().enumerate() {
        let unnamed = format!("OPERATIVE {i}");
        let name = names.get(i).unwrap_or(&unnamed);
        // Only show slots that are actually staffed: trailing headroom would read as phantom operatives.
        if i >= names.len().max(1) && !k.knows_anything() {
            continue;
        }
        rows.extend(roster_rows(name, k));
    }
    rows
}

impl Knowledge {
    /// Does this operative believe anything at all about anything?
    pub fn knows_anything(&self) -> bool {
        Subject::ALL.iter().any(|s| self.knows(*s))
    }
}

fn toggle_roster(
    actions: crate::input::Actions,
    current: Res<State<MenuState>>,
    mut next: ResMut<NextState<MenuState>>,
) {
    // `set_if_neq` is not needed because the target always differs from the source of the branch.
    if actions.just_pressed(crate::input::Action::ToggleRoster) {
        match current.get() {
            MenuState::Closed => next.set(MenuState::Roster),
            MenuState::Roster => next.set(MenuState::Closed),
            _ => {}
        }
    }
}

/// Is the roster overlay open **at the Site**? (FVS-L-6.)
///
/// A plain resource, deliberately **not** a `SubState`. `MenuState` is sourced on
/// `AppState::InGame`, so Bevy removes `State<MenuState>` the moment the app leaves it — which is
/// exactly the crash this item was filed for. Minting a second `SubState` on `AppState::Site` would
/// work, but it would also be a second state machine to keep in step with the first for one boolean.
///
/// In-game the roster is a `MenuState` variant because it belongs to a *stack* of mutually exclusive
/// overlays (pause, settings, controls) that must not open on top of one another. At the Site there is
/// no such stack, so a bool is the whole requirement.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SiteRosterOpen(pub bool);

/// `ToggleRoster` at the Site. Same key, same meaning, different owner.
fn toggle_site_roster(actions: crate::input::Actions, mut open: ResMut<SiteRosterOpen>) {
    if actions.just_pressed(crate::input::Action::ToggleRoster) {
        // `ResMut` deref marks the resource changed, which is what drives spawn/despawn below — so
        // write only on an actual press, never unconditionally.
        open.0 = !open.0;
    }
}

/// Leaving the Site closes it, so re-entering does not restore a panel the player did not ask for.
fn close_site_roster(mut open: ResMut<SiteRosterOpen>) {
    open.set_if_neq(SiteRosterOpen(false));
}

fn site_roster_opened(open: Res<SiteRosterOpen>) -> bool {
    open.is_changed() && open.0
}

fn site_roster_closed(open: Res<SiteRosterOpen>) -> bool {
    // `is_changed()` is also true on the frame the resource is inserted, when `.0` is false — that
    // fires one despawn against zero entities, which is a no-op rather than a special case to code.
    open.is_changed() && !open.0
}

fn site_roster_is_open(open: Res<SiteRosterOpen>) -> bool {
    open.0
}

fn spawn_roster(
    mut commands: Commands,
    theme: Res<crate::ui::theme::UiTheme>,
    fonts: Res<crate::ui::theme::FontAssets>,
    table: Res<SquadKnowledge>,
) {
    // A scrim plus a centred bordered panel — the idiom every other overlay in this game uses
    // (`ui::pause`, `ui::settings_menu`, `ui::controls_screen`). This screen used to be bare text at
    // `PositionType::Absolute` with a hand-picked 20 px offset and **no background at all**, so the
    // roster rendered directly over the live world and was frequently unreadable against it.
    //
    // It deliberately does *not* go into `ui::layout`'s region grid. That grid owns the nine HUD
    // corners for `AppState::{InGame, Site}` panels; this is a blocking overlay at `Z_MENU`, and no
    // overlay uses the grid — the frame does not even exist on some of the screens one can open from.
    commands
        .spawn((
            RosterScreenRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            GlobalZIndex(crate::ui::theme::Z_MENU_DIM),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(theme.space_lg)),
                    row_gap: Val::Px(theme.space_xs),
                    min_width: Val::Px(460.0),
                    max_height: Val::Percent(78.0),
                    ..default()
                },
                BackgroundColor(theme.panel),
                crate::ui::widgets::border_all(theme.panel_border),
                GlobalZIndex(crate::ui::theme::Z_MENU),
            ))
            .with_children(|panel| {
                // Five operatives × every subject × every claim outgrows any screen, so it scrolls
                // rather than running off the bottom edge.
                let mut readout = panel.spawn((
                    RosterReadout,
                    crate::ui::rows::RowPanel::default(),
                    bevy::ui_widgets::ScrollArea::default(),
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(theme.space_xs),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ));
                // Seed the first frame's content so the panel is never blank for a frame.
                let rows = roster_rows_all(&table, &[]);
                readout.with_children(|c| {
                    crate::ui::rows::spawn_rows(c, &theme, &fonts, &rows);
                });

                panel.spawn(crate::ui::widgets::text_colored(
                    &theme,
                    &fonts,
                    "A BELIEF MARKED Told OR Read IS HEARSAY — GO AND SEE FOR YOURSELF",
                    theme.font_body * 0.85,
                    theme.text_muted,
                ));
            });
        });
}

fn update_roster(
    mut commands: Commands,
    theme: Res<crate::ui::theme::UiTheme>,
    fonts: Res<crate::ui::theme::FontAssets>,
    table: Res<SquadKnowledge>,
    mut panels: Query<(Entity, &mut crate::ui::rows::RowPanel), With<RosterReadout>>,
) {
    if !table.is_changed() {
        return;
    }
    let rows = roster_rows_all(&table, &[]);
    for (entity, mut panel) in &mut panels {
        // `sync_rows` is itself a no-op when the rows are unchanged, so this is guarded twice — once
        // on the resource and once on the content. That is the same double guard the string version
        // had (`is_changed` plus `if t.0 != line`), preserved.
        crate::ui::rows::sync_rows(&mut commands, entity, &mut panel, &theme, &fonts, rows.clone());
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
                // Reviewing the roster between expeditions is a real want, and it is now served by the
                // Site-side toggle registered below rather than by widening this one (FVS-L-6).
                toggle_roster.run_if(in_state(AppState::InGame)),
            )
            // ── FVS-L-6: the same overlay, at the Site, on its own state ────────────────────────
            //
            // `spawn_roster` was ALREADY Site-compatible and nobody had noticed: it is a self-contained
            // full-screen scrim at `Z_MENU`, and its own comment records that it deliberately avoids
            // `ui::layout`'s region grid because "the frame does not even exist on some of the screens
            // one can open from". So this needs no second screen, no duplicated rows, and no new
            // widgets — only a trigger that does not depend on `MenuState`, which is the single thing
            // that was actually missing.
            //
            // Reusing the overlay rather than building a Site-native panel is also the choice that
            // keeps ONE roster: two screens rendering the same beliefs would drift, and the whole
            // point of this screen is that what it says is true.
            .init_resource::<SiteRosterOpen>()
            .add_systems(Update, toggle_site_roster.run_if(in_state(AppState::Site)))
            .add_systems(
                Update,
                (
                    spawn_roster.run_if(site_roster_opened),
                    despawn_scoped::<RosterScreenRoot>.run_if(site_roster_closed),
                    update_roster.run_if(site_roster_is_open),
                )
                    .run_if(in_state(AppState::Site)),
            )
            // Leaving the Site tears the overlay down AND clears the flag, so the panel never
            // reappears unasked on the next visit — and never outlives the screen it belongs to.
            .add_systems(
                OnExit(AppState::Site),
                (close_site_roster, despawn_scoped::<RosterScreenRoot>),
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
        use crate::ui::rows::{Cell, Emphasis};
        let mut k = Knowledge::default();
        k.learn(Subject::BearCopies, Claim::Lethal, Provenance::Told, 3);
        let rows = roster_rows("OKAFOR", &k);
        let printed = format!("{rows:?}");
        assert!(printed.contains("LETHAL"), "{printed}");
        assert!(printed.contains("Told"), "provenance must be visible: {printed}");
        // Confidence is a `Bar` now — a LENGTH rather than a percentage string. Cleveland & McGill's
        // ordering puts length above colour and above text for magnitude, which is why `ui::rows`
        // offers the cell at all (`docs/ui.md` §1.3).
        assert!(
            rows.iter().any(|r| r.cells.iter().any(|c| matches!(c, Cell::Bar { .. }))),
            "confidence must be visible: {printed}"
        );

        // STRONGER than the string test it replaces: hearsay is now the LOUD row, not merely a word
        // the player has to read for. That is FVS-O-5's whole counter-play made perceptible.
        let belief = rows
            .iter()
            .find(|r| r.label().is_some_and(|l| l.contains("LETHAL")))
            .expect("the belief is listed");
        assert_eq!(belief.emphasis, Emphasis::Alert, "a rumour is the actionable row");

        let mut firsthand = Knowledge::default();
        firsthand.learn(Subject::BearCopies, Claim::Lethal, Provenance::Firsthand, 3);
        let seen = roster_rows("OKAFOR", &firsthand);
        let seen_row = seen
            .iter()
            .find(|r| r.label().is_some_and(|l| l.contains("LETHAL")))
            .expect("the belief is listed");
        assert_eq!(seen_row.emphasis, Emphasis::Muted, "experience is settled, so it recedes");
    }

    #[test]
    fn an_operative_who_has_met_nothing_says_so_rather_than_rendering_blank() {
        // "Never encountered" is a real state, distinct from "unsure" — the Fisher point the whole
        // model rests on. A blank page would read as a bug.
        let rows = roster_rows("NDIAYE", &Knowledge::default());
        let labels: Vec<&str> = rows.iter().filter_map(|r| r.label()).collect();
        assert!(
            labels.iter().any(|l| l.contains("NO FIELD EXPERIENCE")),
            "{labels:?}"
        );
    }

    #[test]
    fn the_whole_roster_names_every_staffed_operative_and_no_phantoms() {
        // Trailing headroom in `SquadKnowledge::members` must not render as operatives who do not
        // exist — a roster listing eight people for a squad of five is worse than one listing none.
        let mut table = SquadKnowledge::default();
        table.members[0].learn(Subject::Crabs, Claim::Lethal, Provenance::Firsthand, 1);
        let rows = roster_rows_all(&table, &["OKAFOR".to_string()]);
        let labels: Vec<&str> = rows.iter().filter_map(|r| r.label()).collect();
        assert!(labels.contains(&"OKAFOR"));
        assert!(
            !labels.iter().any(|l| l.contains("OPERATIVE 4")),
            "an unstaffed slot with no beliefs must not appear: {labels:?}"
        );
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
