//! **The `CurriculumDirector`** (FVS-H-3) — which world the next expedition gets, chosen from the QD
//! archive by where the Director is *learning fastest*.
//!
//! # This is not the elite overlay
//!
//! `elite_overlay` installs **one** archive cell, named by an env var, for the whole process. That is a
//! development tool: it answers "what does elite 3,4 play like?". This answers a different question —
//! *"what should the next expedition be, given how the last few went?"* — and it answers it again every
//! time the player goes through the ASYNC door.
//!
//! # Grounding: [LPM] argues AGAINST the obvious design
//!
//! Oudeyer & Kaplan (2007), *What is Intrinsic Motivation? A Typology of Computational Approaches*
//! (10.3389/neuro.12.006.2007), §"Maximizing competence progress — aka Flow motivation (CPM)". The
//! backlog's phrasing — "target the learning-progress band" — reads as *pick the middle-difficulty
//! cell*, i.e. a pair of thresholds. The paper considers exactly that and rejects it:
//!
//! > "Flow refers to the state of pleasure related to activities for which difficulty is optimal:
//! > neither too easy nor too difficult. […] a possible manner to model flow would be to introduce two
//! > thresholds defining the zone of optimal difficulty. **Yet, the use of thresholds can be rather
//! > fragile, require hand tuning** and possibly complex adaptive mechanism to update these thresholds
//! > during the robot's lifetime. **Another approach can be taken, which avoids the use of thresholds.
//! > It consists in defining the interestingness of a challenge as the competence progress** that is
//! > experienced as the robot repeatedly tries to achieve it."
//!
//! So a cell is not interesting because its difficulty sits in a band. It is interesting because the
//! player is **getting better at it** — "a challenge for which a robot is bad initially but for which it
//! is rapidly becoming good will be highly rewarding". That removes two hand-tuned constants the
//! threshold design would have needed, which is the whole reason to prefer it.
//!
//! **The paper also requires regions.** Comparing progress across "very different sensorimotor
//! situations" is meaningless, so it groups them into regions `R_n` within which comparison is valid,
//! and notes the boundaries normally have to be learned. Here they do not: **a MAP-Elites archive is
//! already partitioned into cells by behaviour descriptor**, and those cells *are* the `R_n`. The QD
//! structure the search produced for other reasons turns out to supply exactly the precondition this
//! model needs — which is the reason this is worth building on the archive rather than on a difficulty
//! scalar.
//!
//! [GRIP] (Rietveld, Miller & Kiverstein 2017) names the consequence: progress niches "progressively
//! disappear as they become more predictable". A cell the player has mastered stops being chosen *on
//! its own*, with nothing decaying it by hand — which is why [`CellHistory::learning_progress`] falls to
//! zero once performance flattens, high or low.
//!
//! # Determinism
//!
//! **Windowed-only**, and structurally: the pick runs on `OnEnter(AppState::InGame)`, and `AppState`
//! comes from `UiPlugin`, which the deterministic core does not register — the same containment
//! `persist`, the O5 economy and the records office rely on, guarded by
//! `replay::ui_never_leaks_into_deterministic_core`.
//!
//! The **selection itself is reproducible given a seed** (the item's acceptance): ties and the
//! exploration draw come from `rng::seeded(run_seed)`, never from wall time or query order, and the
//! candidate list is sorted by cell before any draw. Same campaign state + same seed ⇒ same world.

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::site::ExpeditionReport;

/// How many recent competence readings a cell keeps.
///
/// Six is two "windows" of three. [LPM] warns that a naive derivative comparing a window at `t` with one
/// at `t-θ` is "in fact nonsense" when the two windows describe different situations — here they cannot,
/// because both windows are *within one cell*, which is the region guarantee the archive gives us. Three
/// per window is the smallest that averages out a single unlucky expedition.
pub const HISTORY: usize = 6;
/// Readings per window. `HISTORY / 2`.
pub const WINDOW: usize = HISTORY / 2;

/// What an unvisited cell is assumed to be worth.
///
/// Optimism under uncertainty, and it is doing real work rather than being a default: a cell with no
/// history has **no** measurable learning progress, so a pure-progress rule would never choose one and
/// the director would spend the whole campaign in whichever cell it happened to start in. [LPM]'s
/// progress niches have to be *discovered* by trying. Set above any achievable real progress so
/// unvisited cells are exhausted first, then never again.
pub const UNVISITED: f32 = f32::INFINITY;

/// One archive cell's recent competence readings — a region `R_n` in [LPM]'s sense.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellHistory {
    /// Oldest first, capped at [`HISTORY`].
    pub recent: Vec<f32>,
}

impl CellHistory {
    /// Record one expedition's competence in this cell.
    pub fn observe(&mut self, competence: f32) {
        self.recent.push(competence);
        if self.recent.len() > HISTORY {
            // Drop the oldest. A progress niche that has closed must be able to fall out of the window,
            // or the director keeps returning to a cell the player mastered ten expeditions ago.
            self.recent.remove(0);
        }
    }

    /// Competence progress: how much better the player has got at this cell lately.
    ///
    /// The difference of two window means, per [LPM]'s smoothed CPM form. `None` until there are enough
    /// readings to have two windows — *not* zero, because "no evidence" and "measured flat" are
    /// different states and collapsing them would make an untried cell look mastered. Same distinction
    /// `knowledge::Knowledge::of` draws with `Option`, for the same reason.
    pub fn learning_progress(&self) -> Option<f32> {
        if self.recent.len() < HISTORY {
            return None;
        }
        let n = self.recent.len();
        let older: f32 = self.recent[n - HISTORY..n - WINDOW].iter().sum::<f32>() / WINDOW as f32;
        let newer: f32 = self.recent[n - WINDOW..].iter().sum::<f32>() / WINDOW as f32;
        Some(newer - older)
    }

    /// Interestingness for selection. Unvisited and under-sampled cells score [`UNVISITED`].
    ///
    /// **Absolute** progress: a cell the player is getting rapidly *worse* at is as informative as one
    /// they are mastering — both mean the difficulty is live rather than settled. Signed progress would
    /// make the director flee anything that starts going badly, which is the opposite of a curriculum.
    pub fn interest(&self) -> f32 {
        self.learning_progress().map_or(UNVISITED, f32::abs)
    }
}

/// How well one expedition went, in `[0, 1]`.
///
/// **The same five terms the Council rates** (`site::o5::ExpeditionReport`), read for a different
/// question. P-1 deliberately gave the search and the Council one shared vocabulary for "how did that
/// expedition go"; this is a third reader of that vocabulary, not a fourth definition of the facts. The
/// Council asks *what funding did this earn*; the director asks *how competent was this*, and those
/// genuinely differ — a cheap win in an easy world funds well and teaches nothing.
pub fn competence(r: &ExpeditionReport) -> f32 {
    if r.squad_size == 0 {
        return 0.0;
    }
    let survival = r.survivors as f32 / r.squad_size as f32;
    // Extraction is the hinge for the Council and it is the hinge here too — a capture you could not
    // walk out with is not a demonstration of competence.
    let extracted = if r.extracted { 1.0 } else { 0.0 };
    // Captures saturate: the second containment proves much less than the first, and an uncapped term
    // would let one lucky world dominate the history.
    let captures = (r.captures as f32 / 2.0).min(1.0);
    let breaches = (r.breaches as f32 / 4.0).min(1.0);
    (0.4 * survival + 0.3 * extracted + 0.3 * captures - 0.2 * breaches).clamp(0.0, 1.0)
}

/// Which world the next expedition gets, and what the campaign has learned about each cell.
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurriculumDirector {
    /// Per-cell competence history. `BTreeMap` so iteration is ordered by cell — the selection must not
    /// depend on hash order, which is the determinism trap this repo has recorded four times.
    pub cells: BTreeMap<String, CellHistory>,
    /// The cell the current expedition was sampled from, so its report can be credited back to it.
    pub current: Option<(usize, usize)>,
}

/// `BTreeMap` key for a cell. A tuple key would serialise to RON as a map with tuple keys, which is
/// awkward to read and diff by hand — and this file is campaign state a human may well inspect.
pub fn key(cell: (usize, usize)) -> String {
    format!("{},{}", cell.0, cell.1)
}

impl CurriculumDirector {
    /// Credit an expedition's outcome to the cell it was sampled from.
    ///
    /// A report with no `current` cell is dropped rather than attributed to a guess: the first
    /// expedition of a campaign predates any pick, and crediting it to an arbitrary cell would poison
    /// that cell's history with an outcome it did not produce.
    pub fn record(&mut self, report: &ExpeditionReport) {
        let Some(cell) = self.current else { return };
        self.cells.entry(key(cell)).or_default().observe(competence(report));
    }

    /// Pick the next expedition's cell from `candidates`.
    ///
    /// `candidates` is the archive's occupied cells. Returns `None` for an empty archive — a caller with
    /// no archive must fall back to the shipped `config.ron`, which is the *authored* world rather than
    /// a degraded one, so there is no second path here to get wrong.
    ///
    /// **Reproducible given `seed`** (the item's acceptance): candidates are sorted, interest is a pure
    /// function of stored history, and the only draw is the tie-break.
    pub fn pick(&self, candidates: &[(usize, usize)], seed: u64) -> Option<(usize, usize)> {
        if candidates.is_empty() {
            return None;
        }
        let mut sorted: Vec<(usize, usize)> = candidates.to_vec();
        // SORT-OK: grid cells are unique in an archive — total by construction.
        sorted.sort_unstable();

        let interest = |c: &(usize, usize)| {
            self.cells.get(&key(*c)).map_or(UNVISITED, CellHistory::interest)
        };
        let best = sorted.iter().map(interest).fold(f32::NEG_INFINITY, f32::max);
        // Every cell within a hair of the best is a legitimate pick. Ties are common and expected —
        // notably at the cold start, where EVERY cell is `UNVISITED` — so the tie-break is the seeded
        // draw rather than "first in sort order", which would walk the archive in a fixed, learnable
        // path and defeat the point.
        let tied: Vec<(usize, usize)> = sorted
            .into_iter()
            .filter(|c| interest(c) >= best || (best.is_infinite() && interest(c).is_infinite()))
            .collect();
        if tied.is_empty() {
            return None;
        }
        let mut rng = crate::rng::seeded(seed);
        use crate::rng::DetRng;
        Some(tied[rng.below(tied.len())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(survivors: u32, captures: u32, extracted: bool) -> ExpeditionReport {
        ExpeditionReport { squad_size: 5, survivors, captures, extracted, breaches: 0 }
    }

    fn history(vals: &[f32]) -> CellHistory {
        let mut h = CellHistory::default();
        for v in vals {
            h.observe(*v);
        }
        h
    }

    #[test]
    fn an_unmeasured_cell_is_not_a_flat_one() {
        // "No evidence" and "measured flat" are different states. Collapsing them would make an untried
        // cell look mastered, and the director would never explore. Same `Option` distinction
        // `knowledge::Knowledge::of` draws.
        assert_eq!(history(&[0.5, 0.5]).learning_progress(), None);
        assert_eq!(history(&[0.5; HISTORY]).learning_progress(), Some(0.0));
        assert_eq!(history(&[0.5, 0.5]).interest(), UNVISITED);
    }

    #[test]
    fn a_cell_the_player_is_mastering_beats_one_they_have_settled_into() {
        // THE property, and it is [LPM]'s CPM rather than a difficulty band: interestingness is the
        // competence PROGRESS, so a cell being learned outranks both a mastered one and a hopeless one.
        let learning = history(&[0.1, 0.15, 0.2, 0.6, 0.7, 0.8]);
        let mastered = history(&[0.9; HISTORY]);
        let hopeless = history(&[0.05; HISTORY]);
        assert!(learning.interest() > mastered.interest());
        assert!(learning.interest() > hopeless.interest());
        // And the two settled cells are equally uninteresting — high competence is not itself a reason
        // to return, which is the difference from "pick what they are good at".
        assert_eq!(mastered.interest(), hopeless.interest());
    }

    #[test]
    fn a_closing_progress_niche_stops_being_chosen_on_its_own() {
        // [GRIP]: "progress niches progressively disappear as they become more predictable." Nothing
        // decays this by hand — the window slides and the derivative falls out.
        let mut h = history(&[0.1, 0.2, 0.3, 0.7, 0.8, 0.9]);
        assert!(h.interest() > 0.3, "still climbing");
        for _ in 0..HISTORY {
            h.observe(0.9); // mastered; performance flat
        }
        assert!(h.interest() < 1.0e-6, "a mastered cell must fall to zero interest: {:?}", h.recent);
    }

    #[test]
    fn losing_ground_is_as_interesting_as_gaining_it() {
        // Absolute progress. Signed progress would make the director flee anything going badly, which
        // is the opposite of a curriculum — a cell the player is getting worse at is live, not settled.
        let up = history(&[0.2, 0.2, 0.2, 0.8, 0.8, 0.8]);
        let down = history(&[0.8, 0.8, 0.8, 0.2, 0.2, 0.2]);
        assert!((up.interest() - down.interest()).abs() < 1.0e-6);
    }

    #[test]
    fn selection_is_reproducible_given_a_seed() {
        // The item's stated acceptance.
        let d = CurriculumDirector::default();
        let cells = vec![(0, 0), (1, 2), (3, 1), (7, 7)];
        let a = d.pick(&cells, 0xC0FFEE);
        let b = d.pick(&cells, 0xC0FFEE);
        assert_eq!(a, b);
        assert!(a.is_some());
        // …and the archive's own ordering must not decide it: the same set, shuffled, picks the same.
        let mut shuffled = cells.clone();
        shuffled.reverse();
        assert_eq!(d.pick(&shuffled, 0xC0FFEE), a, "the pick must not depend on candidate order");
    }

    #[test]
    fn unvisited_cells_are_explored_before_any_measured_one() {
        // Optimism under uncertainty. Without it a pure-progress rule can never choose a cell with no
        // history — there is no progress to measure — and the campaign never leaves where it started.
        let mut d = CurriculumDirector::default();
        d.cells.insert(key((0, 0)), history(&[0.1, 0.2, 0.3, 0.7, 0.8, 0.9])); // strong progress
        let pick = d.pick(&[(0, 0), (5, 5)], 1).expect("a pick");
        assert_eq!(pick, (5, 5), "the untried cell must be tried before the well-understood one");
    }

    #[test]
    fn an_empty_archive_yields_no_pick_rather_than_a_guess() {
        // The caller then runs the AUTHORED world from config.ron. One path: no degraded substitute
        // invented here.
        assert_eq!(CurriculumDirector::default().pick(&[], 7), None);
    }

    #[test]
    fn a_report_with_no_sampled_cell_is_dropped_not_guessed() {
        // The first expedition of a campaign predates any pick. Crediting it to an arbitrary cell would
        // poison that cell with an outcome it did not produce.
        let mut d = CurriculumDirector::default();
        d.record(&report(5, 1, true));
        assert!(d.cells.is_empty());
        d.current = Some((2, 2));
        d.record(&report(5, 1, true));
        assert_eq!(d.cells.len(), 1);
    }

    #[test]
    fn competence_reads_the_councils_terms_but_answers_a_different_question() {
        // A flawless run outscores a wipe, and extraction is the hinge for both readers.
        let flawless = competence(&report(5, 2, true));
        let pyrrhic = competence(&report(1, 2, true));
        let stranded = competence(&report(5, 2, false));
        let wipe = competence(&ExpeditionReport { squad_size: 5, ..Default::default() });
        assert!(flawless > pyrrhic && pyrrhic > wipe);
        assert!(flawless > stranded, "a capture you cannot walk out with is not competence");
        assert!((0.0..=1.0).contains(&flawless) && (0.0..=1.0).contains(&wipe));
        // A squad that never existed is zero, not a division by zero.
        assert_eq!(competence(&ExpeditionReport::default()), 0.0);
    }
}

/// Where the director looks for worlds to sample.
///
/// The **tracked** levels archive. Deliberately a constant rather than an env var: `FVS_LEVELS_ELITE`
/// already exists for "pin one elite for this process", and a second env var meaning "sample from this
/// archive" would be two ways to say which worlds the game may use. If no archive is present the
/// director simply does not fire and the authored `config.ron` world plays — the same one path
/// `pick`'s `None` documents.
pub const ARCHIVE: &str = "assets/config/elites_levels.ron";

/// Sample the next expedition's world and overlay it, before the world is built.
///
/// `OnEnter(RunState::Active)` `.before(RunBuild::World)` — the one moment after the player has
/// committed to an expedition and before anything reads `GameConfig.dungeon`. Both entry points (the
/// title screen's NEW RUN and the Site's ASYNC door) set `RunState::Active`, so hanging off the state
/// rather than off either button means a third entry point gets the director for free.
pub fn pick_next_challenge(
    mut director: ResMut<CurriculumDirector>,
    mut gc: ResMut<crate::config::GameConfig>,
    mut briefing: ResMut<ExpeditionBriefing>,
    seed: Option<Res<crate::session::RunSeed>>,
) {
    let challenges = match crate::elite_overlay::levels_archive_cells(ARCHIVE) {
        Ok(c) => c,
        Err(e) => {
            // Not a fallback: with no archive there is nothing to sample, and the AUTHORED world is the
            // right expedition rather than a degraded one. FVS-H-7 is the spike asking whether that
            // framing survives contact — and its answer is here: the state is written to
            // `ExpeditionBriefing` so FVS-L-4 can SAY so, rather than living in a log line nobody sees
            // in a shipped build. A path the player cannot tell they are on is a second path.
            info!("director: no archive to sample ({e}); playing the authored world");
            briefing.0 = None;
            return;
        }
    };
    let cells: Vec<(usize, usize)> = challenges.iter().map(|c| c.cell).collect();
    // The Branch universe's own seed, so a campaign replays identically — and so two campaigns that
    // reached the same state on different seeds still diverge, which is what makes a seed a universe.
    let seed = seed.map(|s| s.0).unwrap_or(0);
    let Some(cell) = director.pick(&cells, seed) else {
        briefing.0 = None;
        return;
    };
    let chosen = challenges.iter().find(|c| c.cell == cell).copied();
    match crate::elite_overlay::apply_dim(&mut gc, crate::elite_overlay::Dim::Levels, &format!("{ARCHIVE}#{},{}", cell.0, cell.1)) {
        Ok(line) => {
            director.current = Some(cell);
            briefing.0 = chosen.map(|c| Briefing { challenge: c, seed });
            info!("director: {line}");
        }
        // A cell the archive listed but cannot decode is a corrupt archive, not a routine miss. Loud,
        // and `current` stays `None` so the resulting expedition is not credited to a world that never
        // loaded.
        Err(e) => {
            briefing.0 = None;
            error!("director: refusing to sample cell {cell:?} — {e}");
        }
    }
}

/// Credit the finished expedition to the cell it came from.
///
/// `OnEnter(AppState::Debrief)`, `.after(site::review::file_expedition_report)` — the same moment and
/// the same reason: FVS-A-5 tears the world down on leaving `RunState::Active`, which `RETURN TO SITE`
/// does *after* the debrief, so this is the last point at which the report is still true.
pub fn record_expedition(
    mut director: ResMut<CurriculumDirector>,
    standing: Option<Res<crate::site::O5Standing>>,
    tally: Option<Res<crate::site::ExpeditionTally>>,
    survivors: Query<(), With<crate::squad::Unit>>,
    contained: Query<(), With<crate::containment::Contained>>,
    outcome: Option<Res<crate::session::RunOutcome>>,
) {
    let _ = standing;
    let Some(tally) = tally else { return };
    let report = ExpeditionReport {
        squad_size: tally.squad_size,
        survivors: survivors.iter().count() as u32,
        captures: contained.iter().count() as u32,
        extracted: outcome.is_some_and(|o| matches!(*o, crate::session::RunOutcome::Victory)),
        breaches: 0,
    };
    director.record(&report);
}

/// **Windowed-only**, like `PersistPlugin` and `O5Plugin`. The harness never registers it, so
/// `OnEnter(RunState::Active)` keeps exactly the nodes the deterministic core has always had and the
/// goldens cannot move.
pub struct DirectorPlugin;

impl Plugin for DirectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurriculumDirector>()
            .init_resource::<ExpeditionBriefing>()
            .add_systems(
                OnEnter(crate::session::RunState::Active),
                pick_next_challenge.before(crate::session::RunBuild::World),
            )
            .add_systems(
                OnEnter(crate::ui::state::AppState::Debrief),
                record_expedition.after(crate::site::review::file_expedition_report),
            );
    }
}


/// What the next expedition is, for FVS-L-4's briefing to render.
///
/// `None` means **no archive was sampled** — the authored `config.ron` world is playing. That is a
/// state the player must be able to see (FVS-H-7): a campaign that silently alternates between directed
/// and authored worlds is running two paths however the code frames it.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct ExpeditionBriefing(pub Option<Briefing>);

/// The sampled world and the universe it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Briefing {
    pub challenge: crate::elite_overlay::LevelChallenge,
    /// The `RunSeed` — the Branch universe this expedition is in.
    pub seed: u64,
}
