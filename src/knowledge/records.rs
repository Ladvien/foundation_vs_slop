//! **The records office** (FVS-O-4) — the `Read` channel, and the only thing that outlives an operative.
//!
//! # Why a report is a *choice*, not bookkeeping
//!
//! FVS-G-3 decided operatives persist across runs carrying their knowledge, and
//! [`super::roster::sync_squad_knowledge`] makes death lose it **structurally** — the table is rebuilt
//! from the living, so an operative who did not come back contributes nothing and their slot resets.
//!
//! That is what makes a report worth writing. It is **insurance**: a voluntary hedge against your own
//! death, filed while you are alive, readable by whoever replaces you. If knowledge survived death
//! automatically the records office would be a formality; because it does not, filing is a decision the
//! player makes with incomplete information about how the next expedition will go.
//!
//! # The cost is exposure, not currency
//!
//! Filing is free in resources, and deliberately so — charging O5 budget would couple the two economies
//! FVS-P-2 keeps disjoint by kind. The real price is that **a filed report is an attack surface**.
//! [MISPERCEPT]'s distinction is the design here: *"Some are the result of secrets. Some are the result
//! of mere error. Intentionality differentiates the gap that results from a secret from the gap that
//! results from an error."* [`super::gossip`] produces the *error* — honest retelling that degrades.
//! The archive is where the *secret* can be planted, because a written record is exactly the kind of
//! plausible artefact SCP-9191 produces (FVS-O-5). Curating it is the counter-play, which is why
//! [`Records::purge`] exists before anything can seed a lie.
//!
//! # `Read` is the weakest provenance, and that is the point
//!
//! `Provenance::Read` sits below `Told`. An operative who read a write-up believes it less than one who
//! was told by a colleague, who believes it less than one who saw it. So the archive **preserves**
//! knowledge without **replacing** experience — a squad rebuilt entirely from reports is measurably
//! more tentative than the veterans it replaced, which is the right consequence of losing them.

use bevy::prelude::*;

use super::{Claim, Knowledge, Provenance, Subject};
use crate::squad::Unit;

/// One filed write-up.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub subject: Subject,
    pub claim: Claim,
    /// Which operative filed it. Kept so the records screen can attribute a claim, and so FVS-O-5's
    /// planted report can be identified by an author nobody recognises.
    pub author: usize,
    /// Run tick it was filed on.
    pub filed: u64,
}

/// The Site's archive. Meta-progress: not run-scoped, and saved.
#[derive(Resource, Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Records {
    pub filed: Vec<Report>,
}

impl Records {
    /// File one claim, if it is not already on the shelf.
    ///
    /// Deduplicated on `(subject, claim)` rather than on the whole report: two operatives filing the
    /// same finding is one fact, not two, and letting it stack would let a squad manufacture apparent
    /// corroboration out of a single observation.
    pub fn file(&mut self, report: Report) -> bool {
        if self.filed.iter().any(|r| r.subject == report.subject && r.claim == report.claim) {
            return false;
        }
        self.filed.push(report);
        true
    }

    /// Remove every report making a claim. **The curation counter-play** (FVS-O-5).
    ///
    /// Returns how many were pulled, so the UI can say what happened rather than silently succeeding.
    pub fn purge(&mut self, subject: Subject, claim: Claim) -> usize {
        let before = self.filed.len();
        self.filed.retain(|r| !(r.subject == subject && r.claim == claim));
        before - self.filed.len()
    }

    /// Everything the archive says about one subject.
    pub fn about(&self, subject: Subject) -> impl Iterator<Item = &Report> {
        self.filed.iter().filter(move |r| r.subject == subject)
    }
}

/// Write up what the squad currently knows firsthand.
///
/// **Only firsthand findings are filed.** A report is a primary source; letting an operative write up
/// something they were merely *told* would launder hearsay into the archive, and since `Read` beliefs
/// are then handed to the next squad it would let a rumour outlive its own decay — a false belief could
/// circulate forever by being written down. This is the single most important rule in the module.
pub fn file_squad_findings(
    table: &super::SquadKnowledge,
    records: &mut Records,
    tick: u64,
) -> usize {
    let mut filed = 0;
    // `SquadKnowledge` is an array indexed by `SquadMember`, so this walk is already a total order —
    // no query, no sort needed.
    for (member, knowledge) in table.members.iter().enumerate() {
        for subject in Subject::ALL {
            for claim in Claim::ALL {
                let Some(b) = knowledge.of(subject, claim) else { continue };
                if b.provenance != Provenance::Firsthand {
                    continue;
                }
                if records.file(Report { subject, claim, author: member, filed: tick }) {
                    filed += 1;
                }
            }
        }
    }
    filed
}

/// Hand the archive to the squad that just deployed.
///
/// Runs at world construction (`RunBuild::PostPopulate`), after
/// [`super::roster::restore_squad_knowledge`], so a returning veteran's own stronger beliefs are
/// already in place and `learn`'s provenance ordering refuses to downgrade them.
pub fn brief_from_records(
    records: Res<Records>,
    clock: Res<crate::session::RunClock>,
    mut operatives: Query<&mut Knowledge, With<Unit>>,
) {
    for mut knowledge in &mut operatives {
        for report in &records.filed {
            knowledge.learn(report.subject, report.claim, Provenance::Read, clock.ticks);
        }
    }
}

/// The author index a **planted** report carries.
///
/// Outside the roster on purpose (`SquadKnowledge` holds `ROSTER_SLOTS`), so the records screen shows a
/// signature belonging to no operative who has ever served. That is the player-facing tell, and it is
/// the whole reason `Report::author` is stored: [MISPERCEPT]'s point is that **intentionality** is what
/// separates a planted lie from an honest error, and a forged attribution is where the intent shows.
///
/// It is deliberately *findable*, not hidden. The counter-play is curation, and curation you cannot
/// perform is not counter-play.
pub const PHANTOM_AUTHOR: usize = usize::MAX;

/// Ask the archive to carry a lie (FVS-O-5).
///
/// A `Message` rather than a direct call because the **trigger** is SCP-9191's, and that antagonist is
/// FVS-K-4's work. Shipping the mechanism behind a message means the endgame only has to decide *when*
/// — and it means this is fully testable now, rather than blocked on a boss that does not exist.
#[derive(Message, Debug, Clone, Copy)]
pub struct SeedMisinformation {
    pub subject: Subject,
    /// The claim to plant. Callers should plant something the ground truth CONTRADICTS; validated by
    /// [`is_false`] so a "lie" that happens to be true cannot be seeded by accident.
    pub claim: Claim,
}

/// Is this claim actually false about this subject, per the authored ground truth?
///
/// The truth lives in the `research:` curriculum (`HiddenTruth`), which is the same table the research
/// economy converges on — so a planted lie is false in exactly the sense the player can *disprove* by
/// studying the specimen. One source of truth, two consumers.
pub fn is_false(
    curriculum: &crate::research::Curriculum,
    subject: Subject,
    claim: Claim,
) -> Option<bool> {
    let truth = curriculum.truth(subject)?;
    Some(match claim {
        Claim::Lethal => !truth.lethality,
        Claim::Harmless => truth.lethality,
        // "It can be contained" is false only where nothing contains it — i.e. the curriculum has no
        // entry at all, which the `?` above already returned `None` for.
        Claim::Containable => false,
    })
}

/// Plant a forged report.
pub fn seed_misinformation(
    mut asked: MessageReader<SeedMisinformation>,
    curriculum: Option<Res<crate::research::Curriculum>>,
    clock: Option<Res<crate::session::RunClock>>,
    mut records: ResMut<Records>,
) {
    let Some(curriculum) = curriculum else { return };
    let tick = clock.map(|c| c.ticks).unwrap_or(0);
    for req in asked.read() {
        // Refuse to plant something that is TRUE. A "lie" the player could verify and find correct
        // would make the whole detection loop meaningless — and it would quietly turn the antagonist
        // into a source of accurate intelligence.
        match is_false(&curriculum, req.subject, req.claim) {
            Some(true) => {}
            _ => {
                warn!(
                    "records: refusing to plant {:?}/{:?} — it is not false per the authored truth",
                    req.subject, req.claim
                );
                continue;
            }
        }
        if records.file(Report {
            subject: req.subject,
            claim: req.claim,
            author: PHANTOM_AUTHOR,
            filed: tick,
        }) {
            warn!("records: a report signed by no known operative has appeared on the shelf");
        }
    }
}

/// Root marker for the records readout at the Site.
#[derive(Component)]
pub struct RecordsPanel;

#[derive(Component)]
pub struct RecordsReadout;

/// A request to act on the archive, from **either** input route — the key or the panel button.
///
/// Two variants and two readers: [`records_input`] files, `antagonist::curate_archive` purges. Each
/// `MessageReader` has its own cursor, so one message type serving both costs nothing and keeps the
/// click and the key on one path per verb — the `selection::ArmRequest` discipline.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveRequest {
    /// Write the squad's firsthand findings onto the shelf.
    File,
    /// Pull the reports the squad's own experience contradicts.
    Curate,
}

/// The archive readout, as rows.
///
/// `disprovable` is how many filed reports the squad's own firsthand experience contradicts — i.e.
/// what curating would pull right now. Surfaced as a **count of the verb's effect** rather than as a
/// bare keybind, because FVS-L-1's rule applies here too: an unmet clause is an *instruction*.
/// "NOTHING TO CURATE" and "PULL 2 DISPROVEN" are different situations and the player must be able to
/// tell them apart without pressing the key to find out.
///
/// Rows rather than one `\n`-joined `String`. The old form had a single `TextColor`, so a report
/// signed `?? UNATTRIBUTED ??` — the entire tell of the SCP-9191 endgame — rendered in exactly the
/// same ink as the genuine ones. The player was expected to *read* for it. It is now the loud row.
pub fn records_rows(
    records: &Records,
    unfiled: usize,
    disprovable: usize,
) -> Vec<crate::ui::rows::Row> {
    use crate::ui::rows::{Emphasis, Row};
    let mut rows = vec![Row::header("RECORDS OFFICE")];

    if records.filed.is_empty() {
        // Distinct from "nothing to file": an empty archive is a real state, and saying so tells the
        // player the office works rather than leaving them looking at a blank panel.
        rows.push(Row::note("THE ARCHIVE IS EMPTY."));
        return rows;
    }
    if disprovable == 0 && records.filed.iter().any(|r| r.author == PHANTOM_AUTHOR) {
        // The whole point of the endgame, stated as an instruction. A player looking at a report
        // signed by nobody must be told the counter-play is *an expedition*, not a keypress they are
        // missing.
        rows.push(
            Row::note("GO AND SEE THE THING ITSELF; HEARSAY CANNOT EDIT THE ARCHIVE.")
                .with_emphasis(Emphasis::Normal),
        );
    }
    let _ = unfiled; // The counts live on the buttons now — see `archive_button_label`.

    for r in &records.filed {
        // A planted report is named as *unattributed*, not silently rendered with a nonsense index,
        // and it is now the brightest row in the panel. The player has to be able to SEE the thing
        // they are meant to curate.
        let row = if r.author == PHANTOM_AUTHOR {
            Row::unmet(
                format!("{:?}: {:?}", r.subject, r.claim),
                "?? UNATTRIBUTED ??".to_string(),
            )
        } else {
            Row::met(
                format!("{:?}: {:?}", r.subject, r.claim),
                format!("OP {} @ t{}", r.author, r.filed),
            )
        };
        rows.push(row);
    }
    rows
}

/// What one archive button reads. Pure, states its key, and **names the count** so the two verbs are
/// distinguishable before they are pressed.
pub fn archive_button_label(
    req: ArchiveRequest,
    unfiled: usize,
    disprovable: usize,
    bindings: &crate::input::KeyBindings,
) -> String {
    let (action, verb, n) = match req {
        ArchiveRequest::File => (crate::input::Action::FileFindings, "FILE", unfiled),
        ArchiveRequest::Curate => (crate::input::Action::CurateArchive, "PULL", disprovable),
    };
    // The LIVE binding, not `default_binding()`.
    let key = bindings.key_char(action);
    if n == 0 {
        // Never a bare disabled verb. Which *nothing* this is matters: nothing to write up is a
        // different instruction from nothing to disprove.
        let why = match req {
            ArchiveRequest::File => "NOTHING UNWRITTEN",
            ArchiveRequest::Curate => "NOTHING DISPROVEN",
        };
        format!("{key}  {verb} — {why}")
    } else {
        let noun = match req {
            ArchiveRequest::File => "UNWRITTEN FINDING(S)",
            ArchiveRequest::Curate => "REPORT(S) YOU HAVE DISPROVEN",
        };
        format!("{key}  {verb} {n} {noun}")
    }
}

/// One clickable archive button, tagged with the request it sends.
///
/// **Spawned once and never rebuilt**, for the reason `site::review::BuyButton` records:
/// `rows::sync_rows` despawns a panel's children whenever its content changes, and filing changes it,
/// so a button inside that subtree would be destroyed under a cursor mid-click.
#[derive(Component, Clone, Copy)]
pub struct ArchiveButton(pub ArchiveRequest);

/// An archive button's label node, so the counts can be rewritten without respawning the button.
#[derive(Component, Clone, Copy)]
pub struct ArchiveButtonLabel(pub ArchiveRequest);

fn spawn_panel(
    mut commands: Commands,
    theme: Res<crate::ui::theme::UiTheme>,
    fonts: Res<crate::ui::theme::FontAssets>,
    regions: Res<crate::ui::layout::HudRegions>,
) {
    // Parented into the shared region grid rather than absolutely positioned. The Site runs FOUR
    // panels at once (curriculum, research, requisition, records) and each used to claim a corner
    // independently, with no owner able to notice a collision or make room for a fifth.
    let panel = (
            RecordsPanel,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_sm)),
                min_width: Val::Px(340.0),
                ..default()
            },
            BackgroundColor(theme.panel),
            crate::ui::widgets::border_all(theme.panel_border),
            // The container ignores clicks; the buttons below are individually pickable, which is
            // what `Pickable` being per-entity buys.
            Pickable::IGNORE,
    );
    let Some(mut ec) = crate::ui::layout::panel_in(
        &mut commands,
        &regions,
        crate::ui::layout::Region::BottomRight,
        panel,
    ) else {
        error!("records office: no layout frame at spawn — the filing readout is not shown");
        return;
    };
    ec.with_children(|p| {
        // The shelf scrolls: an archive grows without bound across a campaign, and a panel that
        // silently ran off the bottom of the screen would hide exactly the planted report the player
        // is hunting. Same `ScrollArea` + `Overflow::scroll_y` pair `site_hud`/`research_hud` use.
        p.spawn((
            RecordsReadout,
            crate::ui::rows::RowPanel::default(),
            bevy::ui_widgets::ScrollArea::default(),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                max_height: Val::Px(220.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ));

        // The action half: two stable buttons.
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                margin: UiRect::top(Val::Px(theme.space_sm)),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|col| {
            for req in [ArchiveRequest::File, ArchiveRequest::Curate] {
                col.spawn((
                    ArchiveButton(req),
                    bevy::ui_widgets::Button,
                    bevy::picking::hover::Hovered::default(),
                    Node {
                        padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_xs)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(theme.radius)),
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    crate::ui::widgets::border_all(theme.panel_border),
                ))
                .observe(move |_: On<bevy::ui_widgets::Activate>, mut out: MessageWriter<ArchiveRequest>| {
                    out.write(req);
                })
                .with_children(|b| {
                    b.spawn((
                        ArchiveButtonLabel(req),
                        crate::ui::widgets::text_colored(&theme, &fonts, "", theme.font_body, theme.text),
                        Pickable::IGNORE,
                    ));
                });
            }
        });
    });
}

/// How many firsthand findings are not yet on the shelf.
fn unfiled_count(table: &super::SquadKnowledge, records: &Records) -> usize {
    let mut probe = records.clone();
    file_squad_findings(table, &mut probe, 0)
}

fn records_input(
    actions: crate::input::Actions,
    mut requests: MessageReader<ArchiveRequest>,
    table: Res<super::SquadKnowledge>,
    clock: Option<Res<crate::session::RunClock>>,
    mut records: ResMut<Records>,
) {
    // `crate::input::Action` owns the binding; `input::the_key_space_has_no_collisions` is what
    // keeps this key from quietly colliding with another. Every request is drained so an unread one
    // cannot be redelivered next frame and file twice.
    let clicked = requests.read().any(|r| *r == ArchiveRequest::File);
    if clicked || actions.just_pressed(crate::input::Action::FileFindings) {
        let tick = clock.map(|c| c.ticks).unwrap_or(0);
        let n = file_squad_findings(&table, &mut records, tick);
        if n > 0 {
            info!("records: filed {n} finding(s)");
        }
    }
}

fn update_panel(
    mut commands: Commands,
    theme: Res<crate::ui::theme::UiTheme>,
    fonts: Res<crate::ui::theme::FontAssets>,
    records: Res<Records>,
    table: Res<super::SquadKnowledge>,
    bindings: Res<crate::input::KeyBindings>,
    mut panels: Query<(Entity, &mut crate::ui::rows::RowPanel), With<RecordsReadout>>,
    mut labels: Query<(&ArchiveButtonLabel, &mut Text)>,
) {
    // Counted through the SAME function the verb uses, on a clone, so the panel cannot promise a purge
    // the key would not perform. Two implementations of "what is disprovable" would drift, and the one
    // the player reads is the one that would be wrong.
    let mut probe = records.clone();
    let disprovable = crate::antagonist::purge_disproven(&table, &mut probe);
    let unfiled = unfiled_count(&table, &records);
    let rows = records_rows(&records, unfiled, disprovable);
    for (entity, mut panel) in &mut panels {
        crate::ui::rows::sync_rows(&mut commands, entity, &mut panel, &theme, &fonts, rows.clone());
    }
    for (label, mut text) in &mut labels {
        let want = archive_button_label(label.0, unfiled, disprovable, &bindings);
        if text.0 != want {
            text.0 = want;
        }
    }
}

/// Hover + has-anything-to-do styling for the archive buttons. Luminance and border only, never hue.
fn style_archive_buttons(
    theme: Res<crate::ui::theme::UiTheme>,
    records: Res<Records>,
    table: Res<super::SquadKnowledge>,
    mut buttons: Query<(
        &ArchiveButton,
        &bevy::picking::hover::Hovered,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut labels: Query<(&ArchiveButtonLabel, &mut TextColor)>,
) {
    let mut probe = records.clone();
    let disprovable = crate::antagonist::purge_disproven(&table, &mut probe);
    let unfiled = unfiled_count(&table, &records);
    let live = |req: ArchiveRequest| match req {
        ArchiveRequest::File => unfiled > 0,
        ArchiveRequest::Curate => disprovable > 0,
    };
    for (btn, hovered, mut bg, mut border) in &mut buttons {
        let want_bg = if hovered.0 && live(btn.0) {
            theme.panel_border.with_alpha(0.16)
        } else {
            theme.panel
        };
        let want_border =
            if live(btn.0) { theme.panel_border } else { theme.panel_border.with_alpha(0.25) };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want = crate::ui::widgets::border_all(want_border);
        if border.top != want.top {
            *border = want;
        }
    }
    for (label, mut color) in &mut labels {
        let want = if live(label.0) { theme.text } else { theme.text_muted };
        if color.0 != want {
            color.0 = want;
        }
    }
}

/// The records office. Windowed except the briefing, which is world construction.
pub struct RecordsPlugin;

impl Plugin for RecordsPlugin {
    fn build(&self, app: &mut App) {
        use crate::ui::state::{despawn_scoped, AppState};
        app.init_resource::<Records>()
            .add_message::<SeedMisinformation>()
            // The clickable FILE / PULL buttons send this; `records_input` and
            // `antagonist::curate_archive` are its two readers, one verb each.
            .add_message::<ArchiveRequest>()
            .add_systems(Update, seed_misinformation)
            .add_systems(
                OnEnter(crate::session::RunState::Active),
                // AFTER the roster restore, so a returning veteran's firsthand beliefs are already in
                // place and `learn`'s provenance ordering refuses to downgrade them to `Read`.
                brief_from_records
                    .in_set(crate::session::RunBuild::PostPopulate)
                    .after(super::roster::restore_squad_knowledge),
            )
            // **The panel opens itself when you walk in.** The archive is a place: RAISA keeps
            // it, Farrow stands in it, and the reports the antagonist plants appear on its shelves.
            //
            // `Update` and not `OnEnter(AppState::Site)`: the room, not the screen, is what
            // decides now. The key binding is deliberately NOT gated — see the applier systems
            // below, which still run anywhere in the hub. Presence OFFERS; the key still ACTS.
            .add_systems(
                Update,
                spawn_panel
                    .after(crate::ui::layout::spawn_frame)
                    .run_if(crate::site::panel_wanted::<RecordsPanel>(crate::site::AreaId::Records)),
            )
            .add_systems(
                Update,
                despawn_scoped::<RecordsPanel>
                    .run_if(crate::site::panel_stale::<RecordsPanel>(crate::site::AreaId::Records)),
            )
            // Leaving the Site entirely still tears it down, whichever room the player was in.
            .add_systems(OnExit(AppState::Site), despawn_scoped::<RecordsPanel>)
            .add_systems(
                Update,
                (records_input, update_panel, style_archive_buttons)
                    .run_if(in_state(AppState::Site)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::SquadKnowledge;

    fn table_with(provenance: Provenance) -> SquadKnowledge {
        let mut t = SquadKnowledge::default();
        t.members[1].learn(Subject::BearCopies, Claim::Lethal, provenance, 7);
        t
    }

    #[test]
    fn only_firsthand_findings_are_written_up() {
        // THE rule of this module. Filing hearsay would launder a rumour into the archive, and since
        // `Read` beliefs are handed to the next squad, a false belief could then circulate forever by
        // having been written down — outliving the decay that is supposed to kill it.
        let mut records = Records::default();
        assert_eq!(file_squad_findings(&table_with(Provenance::Firsthand), &mut records, 1), 1);

        let mut records = Records::default();
        assert_eq!(file_squad_findings(&table_with(Provenance::Told), &mut records, 1), 0);
        assert_eq!(file_squad_findings(&table_with(Provenance::Read), &mut records, 1), 0);
        assert!(records.filed.is_empty(), "hearsay must never reach the shelf");
    }

    #[test]
    fn the_same_finding_filed_twice_is_one_fact() {
        // Two operatives filing the same observation is one fact. Letting it stack would let a squad
        // manufacture apparent corroboration out of a single sighting.
        let mut t = SquadKnowledge::default();
        t.members[0].learn(Subject::Crabs, Claim::Lethal, Provenance::Firsthand, 1);
        t.members[3].learn(Subject::Crabs, Claim::Lethal, Provenance::Firsthand, 1);
        let mut records = Records::default();
        assert_eq!(file_squad_findings(&t, &mut records, 1), 1);
        assert_eq!(records.filed.len(), 1);
        // ...and re-filing later is a no-op rather than a duplicate.
        assert_eq!(file_squad_findings(&t, &mut records, 99), 0);
    }

    #[test]
    fn purging_pulls_every_report_making_the_claim() {
        // The curation counter-play FVS-O-5 needs. It reports a COUNT so the UI can say what happened
        // rather than silently succeeding.
        let mut records = Records::default();
        records.file(Report { subject: Subject::Parasite, claim: Claim::Harmless, author: 0, filed: 1 });
        records.file(Report { subject: Subject::Parasite, claim: Claim::Lethal, author: 1, filed: 2 });
        assert_eq!(records.purge(Subject::Parasite, Claim::Harmless), 1);
        assert_eq!(records.purge(Subject::Parasite, Claim::Harmless), 0, "purging twice is idempotent");
        assert_eq!(records.about(Subject::Parasite).count(), 1, "the other claim is untouched");
    }

    #[test]
    fn a_report_confers_the_weakest_provenance_so_it_cannot_replace_experience() {
        // The archive PRESERVES knowledge without REPLACING it: a squad rebuilt entirely from reports
        // is measurably more tentative than the veterans it replaced, which is the right consequence
        // of having lost them.
        assert!(
            Provenance::Read.base_confidence() < Provenance::Told.base_confidence(),
            "a write-up must be believed less than a colleague's word"
        );
        let mut veteran = Knowledge::default();
        veteran.learn(Subject::BearCopies, Claim::Lethal, Provenance::Firsthand, 1);
        let before = veteran.of(Subject::BearCopies, Claim::Lethal).expect("held");
        veteran.learn(Subject::BearCopies, Claim::Lethal, Provenance::Read, 2);
        assert_eq!(
            veteran.of(Subject::BearCopies, Claim::Lethal),
            Some(before),
            "reading a write-up must not downgrade what an operative saw"
        );
    }

    #[test]
    fn a_planted_report_is_visibly_unattributed() {
        // The counter-play is CURATION, and curation you cannot perform is not counter-play. The panel
        // must show the forged signature rather than rendering usize::MAX as a plausible index.
        let mut records = Records::default();
        records.file(Report {
            subject: Subject::Parasite,
            claim: Claim::Harmless,
            author: PHANTOM_AUTHOR,
            filed: 3,
        });
        let rows = records_rows(&records, 0, 0);
        let planted = rows
            .iter()
            .find(|r| r.cells.iter().any(|c| matches!(c, crate::ui::rows::Cell::Value(v) if v.contains("UNATTRIBUTED"))))
            .expect("the tell must be visible");
        // STRONGER than the string test it replaces: the forged report is now the *loud* row, not
        // merely present. As one `\n`-joined `Text` node the panel had a single `TextColor`, so the
        // entire tell of the SCP-9191 endgame rendered in the same ink as the genuine reports and the
        // player was expected to read for it.
        assert_eq!(
            planted.emphasis,
            crate::ui::rows::Emphasis::Alert,
            "the forged report must be the brightest thing in the panel"
        );
        let printed = format!("{rows:?}");
        assert!(
            !printed.contains(&PHANTOM_AUTHOR.to_string()),
            "never print the raw sentinel: {printed}"
        );
    }

    #[test]
    fn a_lie_can_be_corrected_by_purging_it_or_by_seeing_for_yourself() {
        // FVS-O-5's acceptance, both routes, at the model level.
        let mut records = Records::default();
        records.file(Report {
            subject: Subject::Parasite,
            claim: Claim::Harmless,
            author: PHANTOM_AUTHOR,
            filed: 1,
        });

        // Route 1 — CURATE: pull it off the shelf, so it stops being briefed to anyone.
        let mut curated = records.clone();
        assert_eq!(curated.purge(Subject::Parasite, Claim::Harmless), 1);
        assert!(curated.filed.is_empty());

        // Route 2 — VERIFY FIRSTHAND: the operative who was briefed the lie meets the thing itself.
        let mut victim = Knowledge::default();
        victim.learn(Subject::Parasite, Claim::Harmless, Provenance::Read, 2);
        assert!(victim.of(Subject::Parasite, Claim::Harmless).is_some(), "the lie took");
        victim.learn(Subject::Parasite, Claim::Lethal, Provenance::Firsthand, 3);
        assert!(
            victim.of(Subject::Parasite, Claim::Harmless).is_none(),
            "experience must displace the contradicting rumour outright, not sit beside it"
        );
        assert_eq!(
            victim.of(Subject::Parasite, Claim::Lethal).expect("learned").provenance,
            Provenance::Firsthand
        );
    }

    #[test]
    fn the_panel_distinguishes_an_empty_archive_from_nothing_to_file() {
        let empty = records_rows(&Records::default(), 0, 0);
        let labels: Vec<&str> = empty.iter().filter_map(|r| r.label()).collect();
        assert!(labels.iter().any(|l| l.contains("THE ARCHIVE IS EMPTY")), "{labels:?}");

        let mut records = Records::default();
        records.file(Report { subject: Subject::Crabs, claim: Claim::Lethal, author: 2, filed: 5 });
        let filled = records_rows(&records, 3, 0);
        let printed = format!("{filled:?}");
        assert!(!printed.contains("THE ARCHIVE IS EMPTY"));
        assert!(printed.contains("OP 2"), "a report must be attributable: {printed}");
    }

    #[test]
    fn each_archive_button_names_its_key_its_verb_and_its_count() {
        // These are clickable now, so `docs/ui.md` §4.2's operability lens applies in both directions:
        // the button must still state the key, and the key must still do what the button says.
        for req in [ArchiveRequest::File, ArchiveRequest::Curate] {
            let b = crate::input::KeyBindings::default();
            let idle = archive_button_label(req, 0, 0, &b);
            let busy = archive_button_label(req, 3, 2, &b);
            for l in [&idle, &busy] {
                assert!(!l.trim().is_empty());
                let key = l.chars().next().expect("non-empty");
                assert!(key.is_ascii_alphanumeric(), "{l} must lead with its key");
            }
            assert_ne!(idle, busy, "{req:?} reads the same whether or not there is work");
            assert!(busy.contains(if req == ArchiveRequest::File { "3" } else { "2" }), "{busy}");
        }
    }

    #[test]
    fn an_idle_verb_says_which_nothing_it_is() {
        // "Nothing to write up" and "nothing to disprove" are different situations with different
        // responses, and FVS-L-1's rule is that an unmet condition is an INSTRUCTION. A shared
        // greyed-out label would collapse them.
        let b = crate::input::KeyBindings::default();
        let file = archive_button_label(ArchiveRequest::File, 0, 0, &b);
        let curate = archive_button_label(ArchiveRequest::Curate, 0, 0, &b);
        assert!(file.contains("NOTHING UNWRITTEN"), "{file}");
        assert!(curate.contains("NOTHING DISPROVEN"), "{curate}");
        assert_ne!(file, curate);
    }

    #[test]
    fn a_planted_report_with_nothing_disproven_names_the_route_out() {
        // The endgame's instruction. A player staring at a signature belonging to nobody must be told
        // the counter-play is an expedition, not a keypress they have failed to find.
        let mut records = Records::default();
        records.file(Report {
            subject: Subject::Parasite,
            claim: Claim::Harmless,
            author: PHANTOM_AUTHOR,
            filed: 1,
        });
        let rows = records_rows(&records, 0, 0);
        let labels: Vec<&str> = rows.iter().filter_map(|r| r.label()).collect();
        assert!(
            labels.iter().any(|l| l.contains("GO AND SEE THE THING ITSELF")),
            "the route out must be stated: {labels:?}"
        );
    }
}
