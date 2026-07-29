//! **SCP-9191, the generator** (FVS-K-4) — the endgame's first phase.
//!
//! # What the antagonist actually does
//!
//! 9191 is not a monster you shoot. It is a generator whose output *is* the uncanny valley, and the
//! backlog's framing of the endgame is that research "cashes out as restoring curation/quality against
//! an out-of-control generator." Taken literally, that is not a boss arena — it is **an argument about
//! the archive**, and the mechanism for it already shipped: FVS-O-4 built the records office and
//! FVS-O-5 built [`knowledge::records::seed_misinformation`] behind a `Message` precisely so the
//! endgame would only have to decide *when*.
//!
//! This module is the *when*, and the counter-play that makes it a fight.
//!
//! ## The loop
//!
//! 1. The Director completes the curriculum — every goal subject researched. 9191 **wakes**.
//! 2. On each return to Site-67 it files one report **nobody wrote**, carrying a claim the authored
//!    ground truth contradicts. The archive is briefed to every later squad ([`brief_from_records`]),
//!    so an unchallenged lie propagates into operatives who will then act on it.
//! 3. The Director **curates**: `J` at the Site pulls every filed report that the squad's own
//!    *firsthand* experience contradicts.
//!
//! That third step is the whole design. Purging is not a button that deletes suspicious rows — it is
//! gated on **having gone and looked**. You cannot curate what you have not verified, so denying 9191
//! means running expeditions to acquire firsthand knowledge of the things it is lying about. The
//! endgame therefore *drives* the core loop rather than replacing it, which is what the backlog means
//! by research cashing out.
//!
//! ## What is deliberately NOT here yet
//!
//! **The manifestation.** The Director chose "curation phase, then a manifestation", and the
//! manifestation is a field creature — new `FixedUpdate` nodes, a permuted schedule, moved goldens, and
//! therefore a re-bake of every archive. Landing it before FVS-H-1's bake would pay that 12–20 h twice,
//! which is the identical mistake the backlog records under H-1 ("retraining first bakes an archive
//! optimised against an objective that ignores captures, and I-1 then invalidates it").
//!
//! So the seam is [`Antagonist::confrontation_due`] — a real, tested predicate over real state — rather
//! than a `Phase::Manifest` variant that exists and does nothing. A dead enum variant is the "shipped a
//! mechanism, nobody can reach it" shape this repo keeps catching; a predicate that reads `true` and
//! has no consumer yet is honestly incomplete instead.
//!
//! # Determinism
//!
//! Windowed-only. Everything here is gated on `AppState::Site`, which the headless harness never
//! enters, and the lie is chosen by a **fixed scan** over `Subject::ALL × Claim::ALL` rather than by
//! RNG — so there is no seeded draw to get wrong and nothing can reach `snapshot_hash`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::knowledge::records::{is_false, Records, SeedMisinformation, PHANTOM_AUTHOR};
use crate::knowledge::{Claim, Provenance, SquadKnowledge, Subject};
use crate::research::{Curriculum, Researched};
use crate::ui::state::AppState;

/// How many unchallenged lies 9191 must have on the shelf before the arc escalates.
///
/// Four rather than one: a single planted report the Director happens not to notice should not end the
/// campaign, and the arc needs room for the player to fall behind and catch up. It is the count of
/// *unpurged* reports, so it measures the argument's standing rather than 9191's effort.
pub const CONFRONTATION_AT: u32 = 4;

/// What SCP-9191 is currently doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Phase {
    /// Not yet revealed. The player has seen its *output* — the bear copies, the defect the analyst
    /// notices in `slop_pattern` — but nothing has come for the archive.
    #[default]
    Dormant,
    /// Awake, and writing to the shelf.
    Curating,
}

/// SCP-9191's campaign state.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Antagonist {
    pub phase: Phase,
    /// Reports it has planted, ever.
    pub seeded: u32,
    /// Reports the Director has pulled, ever.
    pub purged: u32,
}

impl Antagonist {
    /// Lies currently standing unchallenged.
    ///
    /// Saturating because curation is idempotent and a Director who purges a report 9191 never planted
    /// (a genuine mistake by a dead operative, say) would otherwise underflow into a huge pressure.
    pub fn standing(&self) -> u32 {
        self.seeded.saturating_sub(self.purged)
    }

    /// Has the argument gone badly enough to force the confrontation?
    ///
    /// **The seam for the manifestation** (see the module docs). Tested and reachable now; its consumer
    /// lands after FVS-H-1's bake, because a field creature moves the goldens and would invalidate it.
    pub fn confrontation_due(&self) -> bool {
        self.phase == Phase::Curating && self.standing() >= CONFRONTATION_AT
    }
}

/// Wake 9191 once the curriculum is finished.
///
/// **The threshold is the curriculum's own goal, not a count of anything.** FVS-F-3 authored the graph
/// backwards from SCP-150 so that finishing it *means* something; reusing that as the endgame trigger
/// keeps one definition of "the Foundation has learned what there is to learn here" rather than minting
/// a second one that could disagree with the tech-tree HUD the player is reading.
pub fn wake_on_curriculum_complete(
    mut nine: ResMut<Antagonist>,
    curriculum: Option<Res<Curriculum>>,
    specimens: Query<(&crate::containment::Specimen, Option<&Researched>)>,
) {
    if nine.phase != Phase::Dormant {
        return;
    }
    let Some(curriculum) = curriculum else { return };
    let goals = curriculum.goals();
    if goals.is_empty() {
        return;
    }
    let done = |s: Subject| {
        specimens.iter().any(|(spec, r)| spec.subject == s && r.is_some())
    };
    if goals.iter().copied().all(done) {
        nine.phase = Phase::Curating;
        warn!(
            "SCP-9191: the curriculum is complete, and something has started writing to the archive"
        );
    }
}

/// File one report nobody wrote.
///
/// Chosen by a **fixed scan** rather than a draw: the first `(subject, claim)` the authored ground truth
/// contradicts and that is not already on the shelf. Deterministic by construction, and it means the
/// lies arrive in a stable, authorable order rather than a random one the player cannot learn.
pub fn seed_a_lie(
    mut nine: ResMut<Antagonist>,
    curriculum: Option<Res<Curriculum>>,
    records: Res<Records>,
    mut out: MessageWriter<SeedMisinformation>,
) {
    if nine.phase != Phase::Curating {
        return;
    }
    let Some(curriculum) = curriculum else { return };
    for subject in Subject::ALL {
        for claim in Claim::ALL {
            if is_false(&curriculum, subject, claim) != Some(true) {
                continue;
            }
            if records.filed.iter().any(|r| r.subject == subject && r.claim == claim) {
                continue;
            }
            out.write(SeedMisinformation { subject, claim });
            nine.seeded += 1;
            return;
        }
    }
}

/// Pull every filed report the squad's **firsthand** experience contradicts.
///
/// Returns how many were pulled.
///
/// # Why firsthand, and only firsthand
///
/// This is the counter-play FVS-O-5 names: *you verify against a degraded rumour and you curate against
/// a planted one*. Curating on anything weaker would break the loop in both directions —
///
/// * **`Told` would let a rumour purge the truth.** O-3's retellings degrade but stay in the table, so
///   a squad that had merely heard "the bear is lethal" could pull the *correct* report saying it is
///   not. The archive would then be edited by gossip, which is the failure O-4 exists to prevent.
/// * **`Read` would make the archive edit itself.** `Read` beliefs come *from* the archive, so a
///   planted lie briefed onto the squad would authorise purging the report that contradicts it — 9191
///   would curate on the Director's behalf, and the more it lied the more it could erase.
///
/// So the price of curation is an expedition. That is what makes the endgame drive the core loop.
pub fn purge_disproven(table: &SquadKnowledge, records: &mut Records) -> usize {
    let mut pulled = 0;
    // Collect first: `purge` mutates the shelf, and deciding while iterating it would skip rows.
    let targets: Vec<(Subject, Claim)> = records
        .filed
        .iter()
        .filter(|r| {
            let Some(counter) = r.claim.contradicts() else {
                // `Containable` has no opposite — nothing can disprove "there is a way to contain it",
                // and `is_false` already refuses to plant it, so a filed one is genuine.
                return false;
            };
            table.members.iter().any(|k| {
                k.of(r.subject, counter)
                    .is_some_and(|b| b.provenance == Provenance::Firsthand)
            })
        })
        .map(|r| (r.subject, r.claim))
        .collect();
    for (subject, claim) in targets {
        pulled += records.purge(subject, claim);
    }
    pulled
}

/// Curate the archive against what the squad has actually seen — the key, or the panel's PULL button.
pub fn curate_input(
    actions: crate::input::Actions,
    mut requests: MessageReader<crate::knowledge::records::ArchiveRequest>,
    table: Res<SquadKnowledge>,
    mut records: ResMut<Records>,
    mut nine: ResMut<Antagonist>,
) {
    // `crate::input::Action` owns the binding; `input::the_key_space_has_no_collisions` is what
    // keeps this key from quietly colliding with another. The button routes through the same message
    // rather than calling `purge_disproven` itself, so this stays the single caller — the discipline
    // `selection::ArmRequest` set. Every request is drained so an unread one cannot purge twice.
    let clicked = requests
        .read()
        .any(|r| *r == crate::knowledge::records::ArchiveRequest::Curate);
    if !clicked && !actions.just_pressed(crate::input::Action::CurateArchive) {
        return;
    }
    let n = purge_disproven(&table, &mut records);
    if n == 0 {
        info!(
            "records: nothing on the shelf is contradicted by firsthand experience — go and look at \
             the thing itself"
        );
        return;
    }
    nine.purged = nine.purged.saturating_add(n as u32);
    info!("records: pulled {n} report(s) the squad has personally disproven");
}

/// Does the archive currently hold a report nobody signed?
///
/// Read by `dialogue::triggers::on_unattributed_report` — kept here so "what an unsourced report is"
/// has one definition rather than a `PHANTOM_AUTHOR` comparison copied into every consumer.
pub fn holds_unattributed(records: &Records) -> bool {
    records.filed.iter().any(|r| r.author == PHANTOM_AUTHOR)
}

pub struct AntagonistPlugin;

impl Plugin for AntagonistPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Antagonist>()
            .add_systems(
                OnEnter(AppState::Site),
                // Wake first, so the expedition that *completes* the curriculum is also the one whose
                // homecoming carries the first planted report. A one-visit gap would read as the
                // antagonist politely waiting.
                (wake_on_curriculum_complete, seed_a_lie).chain(),
            )
            .add_systems(Update, curate_input.run_if(in_state(AppState::Site)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{Knowledge, Report};

    fn shelf(rows: &[(Subject, Claim, usize)]) -> Records {
        Records {
            filed: rows
                .iter()
                .map(|&(subject, claim, author)| Report { subject, claim, author, filed: 0 })
                .collect(),
        }
    }

    fn squad_that_saw(subject: Subject, claim: Claim, how: Provenance) -> SquadKnowledge {
        let mut table = SquadKnowledge::default();
        let mut k = Knowledge::default();
        k.learn(subject, claim, how, 1);
        table.members[0] = k;
        table
    }

    #[test]
    fn firsthand_experience_pulls_the_report_it_contradicts() {
        // The counter-play, end to end: 9191 files "the parasite is harmless", an operative meets one
        // and learns otherwise, and the lie comes off the shelf.
        let mut records = shelf(&[(Subject::Parasite, Claim::Harmless, PHANTOM_AUTHOR)]);
        let table = squad_that_saw(Subject::Parasite, Claim::Lethal, Provenance::Firsthand);
        assert_eq!(purge_disproven(&table, &mut records), 1);
        assert!(records.filed.is_empty());
    }

    #[test]
    fn hearsay_cannot_curate_the_archive() {
        // THE test this module exists to make pass, and both halves matter.
        //
        // `Told`: O-3's retellings degrade but persist, so a squad that merely *heard* something could
        // otherwise pull a correct report — the archive edited by gossip, which is the failure FVS-O-4
        // was built to prevent.
        let mut records = shelf(&[(Subject::Parasite, Claim::Harmless, 2)]);
        let told = squad_that_saw(Subject::Parasite, Claim::Lethal, Provenance::Told);
        assert_eq!(purge_disproven(&told, &mut records), 0, "a rumour may not edit the archive");

        // `Read`: these beliefs come FROM the archive, so a planted lie briefed onto the squad would
        // authorise purging whatever contradicts it — 9191 curating on the Director's behalf, erasing
        // more the more it lied.
        let read = squad_that_saw(Subject::Parasite, Claim::Lethal, Provenance::Read);
        assert_eq!(purge_disproven(&read, &mut records), 0, "the archive may not edit itself");
        assert_eq!(records.filed.len(), 1);
    }

    #[test]
    fn curation_leaves_reports_nothing_disproves() {
        // A true report must survive contact with the curation verb, or the counter-play becomes "purge
        // everything" and the archive stops meaning anything.
        let mut records = shelf(&[(Subject::Parasite, Claim::Lethal, 1)]);
        let table = squad_that_saw(Subject::Parasite, Claim::Lethal, Provenance::Firsthand);
        assert_eq!(purge_disproven(&table, &mut records), 0);
        assert_eq!(records.filed.len(), 1, "agreeing with the shelf is not grounds to clear it");
    }

    #[test]
    fn a_containable_report_is_never_purged() {
        // `Containable` has no opposite, so nothing can disprove it — and `is_false` already refuses to
        // plant one, so a filed one is genuine by construction.
        let mut records = shelf(&[(Subject::ComfortBlob, Claim::Containable, 0)]);
        let table = squad_that_saw(Subject::ComfortBlob, Claim::Lethal, Provenance::Firsthand);
        assert_eq!(purge_disproven(&table, &mut records), 0);
    }

    #[test]
    fn the_confrontation_needs_both_a_woken_antagonist_and_standing_lies() {
        let mut nine = Antagonist { phase: Phase::Dormant, seeded: 99, purged: 0 };
        assert!(!nine.confrontation_due(), "a dormant 9191 cannot be winning an argument");
        nine.phase = Phase::Curating;
        assert!(nine.confrontation_due());

        // And curation buys it back.
        nine.purged = 99;
        assert!(!nine.confrontation_due(), "curation must be able to pull the arc back from the edge");
        assert_eq!(nine.standing(), 0);
    }

    #[test]
    fn purging_more_than_was_seeded_does_not_underflow_into_a_crisis() {
        // A Director may legitimately pull a report 9191 never planted — a dead operative's honest
        // mistake, say. Wrapping here would read as the endgame firing for no reason.
        let nine = Antagonist { phase: Phase::Curating, seeded: 1, purged: 5 };
        assert_eq!(nine.standing(), 0);
        assert!(!nine.confrontation_due());
    }
}
