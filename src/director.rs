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

/// Fewest readings a cell needs before it can estimate progress at all.
///
/// One window rather than two (FVS-H-5). Two full windows is the *designed* estimator and is what a
/// cell uses once it has them; requiring them before reporting anything was the larger of the two
/// terms that made exploration unaffordable — see [`CellHistory::learning_progress`].
pub const MIN_READINGS: usize = WINDOW;

/// Optimism budget for a cell with no readings at all.
///
/// **Finite, and decaying with the reading count** (FVS-H-5). This used to be `f32::INFINITY`, on the
/// sound reasoning that a cell with no history has no measurable learning progress, so a pure-progress
/// rule would never choose one and the director would never leave wherever it started — [LPM]'s
/// progress niches have to be *discovered*. That reasoning is right; unbounded optimism was the wrong
/// implementation of it. An infinite score makes every untried cell beat every measured one
/// *absolutely*, so exploration and exploitation run in sequence rather than interleaving, and the
/// exploration phase was measured at **exactly `cells × HISTORY` = 330 expeditions** against a
/// campaign of 10-30. The director was, in effect, a uniform random sampler.
///
/// The standard remedy is a **count-based exploration bonus**: optimism proportional to `1/(1+n)`, so
/// an untried cell is preferred but a genuinely interesting measured one can outrank it. (Strehl &
/// Littman 2008's count-based exploration, as surveyed in Baker et al. 2019,
/// `10.48550/arXiv.1909.07528` — which also names this exact failure: intrinsically-motivated agents
/// "are incentivized to explore uniformly".)
///
/// Set to the largest achievable real progress (a difference of means of competence in `[0, 1]`, so
/// `|progress| <= 1`). An untried cell therefore outscores every measured cell **except** one showing
/// a near-perfect swing — competence averaging ~0 across one window and ~1 across the next — which is
/// precisely the cell worth interrupting exploration for. The ordering [LPM] wants is preserved; it
/// just stops being absolute, which is the whole point.
pub const PRIOR: f32 = 1.0;

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
    /// The difference of two window means, per [LPM]'s smoothed CPM form. `None` below
    /// [`MIN_READINGS`] — *not* zero, because "no evidence" and "measured flat" are different states
    /// and collapsing them would make an untried cell look mastered. Same distinction
    /// `knowledge::Knowledge::of` draws with `Option`, for the same reason.
    ///
    /// **Provisional below a full [`HISTORY`]** (FVS-H-5). The estimator used to refuse to report until
    /// it had two *full* windows, i.e. 6 readings — which, multiplied across a 55-cell archive, was the
    /// larger half of a 330-expedition exploration phase. It now splits whatever it has symmetrically:
    /// with `n` readings it compares the newest `w = min(n/2, WINDOW)` against the `w` before them. At
    /// `n = HISTORY` that is byte-identical to the old computation (`w == WINDOW`, both windows full),
    /// so a fully-sampled cell is scored exactly as before; only the warm-up is now informative rather
    /// than silent. A 1-vs-1 comparison at `n = 3` is noisy, and that is the intended trade — it is
    /// weighed against a decaying [`PRIOR`] that still favours the untried.
    pub fn learning_progress(&self) -> Option<f32> {
        let n = self.recent.len();
        if n < MIN_READINGS {
            return None;
        }
        let w = (n / 2).min(WINDOW);
        let mean = |s: &[f32]| s.iter().sum::<f32>() / s.len() as f32;
        let older = mean(&self.recent[n - 2 * w..n - w]);
        let newer = mean(&self.recent[n - w..]);
        Some(newer - older)
    }

    /// Interestingness for selection: measured progress plus a decaying optimism bonus.
    ///
    /// **Absolute** progress: a cell the player is getting rapidly *worse* at is as informative as one
    /// they are mastering — both mean the difficulty is live rather than settled. Signed progress would
    /// make the director flee anything that starts going badly, which is the opposite of a curriculum.
    ///
    /// The bonus is [`PRIOR`]`/(1 + readings)` — count-based optimism, finite so that exploration and
    /// exploitation **interleave** instead of running in sequence (FVS-H-5). An untried cell scores a
    /// full `PRIOR`, which still beats any measured cell, so the discovery [LPM] requires is preserved;
    /// but a cell showing real progress now overtakes one that has merely been sampled a few times,
    /// which is what makes the exploitation phase reachable inside a real campaign.
    pub fn interest(&self) -> f32 {
        self.learning_progress().map_or(0.0, f32::abs) + optimism(self.recent.len())
    }
}

/// The count-based exploration bonus for a cell with `readings` observations.
///
/// Free function rather than a method so [`CurriculumDirector::pick`] can score a cell with **no**
/// [`CellHistory`] entry at all through the same one path, rather than a second constant that could
/// drift from this one.
pub fn optimism(readings: usize) -> f32 {
    PRIOR / (1.0 + readings as f32)
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

        // A cell with no `CellHistory` entry scores the same as one with an empty history — zero
        // readings, full optimism — through `optimism(0)` rather than a separate constant.
        let interest = |c: &(usize, usize)| {
            self.cells.get(&key(*c)).map_or_else(|| optimism(0), CellHistory::interest)
        };
        let best = sorted.iter().map(interest).fold(f32::NEG_INFINITY, f32::max);
        // Every cell within a hair of the best is a legitimate pick. Ties are common and expected —
        // notably at the cold start, where every cell scores `optimism(0)` — so the tie-break is the
        // seeded draw rather than "first in sort order", which would walk the archive in a fixed,
        // learnable path and defeat the point.
        //
        // The old `best.is_infinite()` escape hatch is gone with `UNVISITED` (FVS-H-5): `>= best` is
        // exact for the equal scores a tie is made of, and `INFINITY >= INFINITY` was the only case it
        // ever existed to cover.
        let tied: Vec<(usize, usize)> = sorted.into_iter().filter(|c| interest(c) >= best).collect();
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
        // Since FVS-H-5 the under-sampled cell no longer scores `INFINITY`; it scores its decaying
        // optimism bonus. The property that matters is unchanged and is asserted directly: a cell with
        // too little evidence still outranks one measured genuinely flat.
        assert!(
            history(&[0.5, 0.5]).interest() > history(&[0.5; HISTORY]).interest(),
            "no evidence must still beat measured-flat, or the director stops exploring"
        );
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
        // The PROGRESS term is what must vanish. Since FVS-H-5 `interest` also carries a decaying
        // optimism bonus, so a mastered cell floors at `optimism(HISTORY)` rather than at zero — it is
        // the least interesting a cell can be, which is what this test is about, and asserting it
        // exactly also pins that a saturated cell's bonus stops decaying with the sliding window.
        assert!(
            h.learning_progress().is_some_and(|p| p.abs() < 1.0e-6),
            "a mastered cell must show no progress: {:?}",
            h.recent
        );
        assert!(
            (h.interest() - optimism(HISTORY)).abs() < 1.0e-6,
            "a mastered cell must fall to the optimism floor: {} vs {}",
            h.interest(),
            optimism(HISTORY)
        );
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

    /// **FVS-H-5's falsification, run as arithmetic — and now the regression test for its remedy.**
    ///
    /// The spike asked: does `UNVISITED = INFINITY` starve the measured cells at the shipped archive
    /// size? Measured answer 2026-07-31: yes, and 6× worse than the entry guessed. The entry estimated
    /// "55 expeditions of pure exploration" (one visit per occupied cell); the real rule was **exactly
    /// `cells × HISTORY` = 330**, because a cell scored `UNVISITED` until it had *two full windows* of
    /// readings and left the infinite tie the moment it graduated, so no pick was ever "wasted" and the
    /// phase was not even stochastic.
    ///
    /// **Both terms were then fixed (2026-07-31).** `MIN_READINGS` dropped to one window, and
    /// `UNVISITED` became a finite decaying [`optimism`] bonus. Under the *flat* competence this
    /// simulation feeds, measured progress is identically zero, so optimism alone orders the cells and
    /// it decreases monotonically in the reading count — the picks are still a strict round-robin, and
    /// the floor is now exactly `cells × MIN_READINGS`.
    ///
    /// ⚠️ **That flatness is the point of the companion test below**, and the reason this number alone
    /// would be a misleading measure of the fix: the real gain is not the halved floor, it is that
    /// exploration and exploitation now *interleave* at all, which a constant-competence simulation
    /// cannot show by construction.
    #[test]
    fn the_unvisited_bonus_starves_exploitation_at_the_shipped_archive_size() {
        // 55 = the occupied-cell count of the shipped `elites_levels.ron` the H-5 entry cites.
        let candidates: Vec<(usize, usize)> = (0..55).map(|i| (i / 8, i % 8)).collect();
        let mut runs = Vec::new();
        for trial in 0..10u64 {
            let mut d = CurriculumDirector::default();
            let mut expeditions = 0u64;
            loop {
                let cell = d
                    .pick(&candidates, trial.wrapping_mul(0x9E37_79B9) ^ expeditions)
                    .expect("candidates are non-empty");
                d.cells.entry(key(cell)).or_default().observe(0.5);
                expeditions += 1;
                let all_measured = candidates
                    .iter()
                    .all(|c| d.cells.get(&key(*c)).is_some_and(|h| h.learning_progress().is_some()));
                if all_measured {
                    break;
                }
                assert!(expeditions < 20_000, "exploration never completed — rule change?");
            }
            runs.push(expeditions);
        }
        // Was `55 × HISTORY(6)` = 330 before the H-5 remedy; now `55 × MIN_READINGS(3)` = 165 under
        // flat competence, for the reason in this test's doc comment. If a future change moves this in
        // EITHER direction, fail so FVS-H-5 gets re-measured rather than silently re-tuned.
        assert!(
            runs.iter().all(|&e| e == 55 * MIN_READINGS as u64),
            "exploration floor is no longer exactly cells×MIN_READINGS ({runs:?}) — \
             re-measure FVS-H-5 and update its entry"
        );
    }

    /// **The property the FVS-H-5 remedy actually buys**, which the flat-competence floor above cannot
    /// show: exploration and exploitation now **interleave**.
    ///
    /// Under `UNVISITED = INFINITY` this was impossible by construction — every untried cell beat every
    /// measured cell *absolutely*, so no measured cell could be revisited until all 55 had been sampled
    /// six times. A finite [`PRIOR`] makes the comparison a real one, so a cell showing genuine learning
    /// progress can be returned to while most of the archive is still untouched. That is the difference
    /// between a curriculum and a shuffle, and it is what makes the director's exploitation phase
    /// reachable inside a 10-30 expedition campaign.
    #[test]
    fn a_cell_showing_real_progress_is_revisited_before_exploration_finishes() {
        let candidates: Vec<(usize, usize)> = (0..55).map(|i| (i / 8, i % 8)).collect();
        let mut d = CurriculumDirector::default();
        // One cell mid-climb: three readings, steeply improving. At `n = 3` the estimator compares one
        // reading against one (`w = min(n/2, WINDOW) = 1`), so progress is 0.8 − 0.5 = 0.3 and interest
        // is 0.3 + optimism(3) = 0.55 — below a pristine cell's 1.0, but well above a cell sampled flat
        // the same number of times. Under the old rule it scored a finite number against INFINITY and
        // could not be picked at all until every other cell had been exhausted six times over.
        d.cells.insert(key((0, 0)), history(&[0.2, 0.5, 0.8]));

        // Sample a slice of the archive flat, as an early campaign would.
        for c in candidates.iter().take(20).skip(1) {
            let h = d.cells.entry(key(*c)).or_default();
            for _ in 0..MIN_READINGS {
                h.observe(0.5);
            }
        }

        let climbing = d.cells[&key((0, 0))].interest();
        let flat = d.cells[&key(candidates[1])].interest();
        assert!(
            climbing > flat,
            "a climbing cell must outrank a flat one at equal sample counts: {climbing} vs {flat}"
        );
        // …and the archive is still overwhelmingly untried, which is the whole point: the director does
        // not have to finish exploring to act on what it has learned.
        let untried = candidates.iter().filter(|c| !d.cells.contains_key(&key(**c))).count();
        assert!(untried > 30, "the simulation should leave most cells untried, got {untried}");
    }

    /// **FVS-H-4's falsification, run as arithmetic.** The spike asked: does absolute progress make
    /// the director *park* in a cell the player is steadily losing at ("the game keeps sending me
    /// back to the thing beating me")? Measured answer: yes — for exactly as long as the decline is
    /// still steepening or moving, and it leaves within one window of the player flatlining.
    ///
    /// That is both halves of the H-4 entry at once: the parking IS the designed reading (a cell the
    /// player is getting worse at is live, not settled), and the exit exists but only at the floor —
    /// the director stops returning once the player is fully crushed (progress → 0 at competence 0),
    /// not before. Whether that reads as "curriculum" or "punishment" is a playtest judgment; this
    /// pins what the rule DOES so that judgment is made about the real behaviour.
    #[test]
    fn a_declining_cell_holds_the_director_until_the_decline_bottoms_out() {
        let mut d = CurriculumDirector::default();
        // One mastered cell (settled, interest 0) and one in steady decline (interest |Δ| = 0.3).
        d.cells.insert(key((0, 0)), history(&[1.0; HISTORY]));
        d.cells.insert(key((1, 1)), history(&[1.0, 0.9, 0.8, 0.7, 0.6, 0.5]));
        let candidates = [(0, 0), (1, 1)];
        let mut consecutive = 0u32;
        let mut level = 0.5f32;
        loop {
            let pick = d.pick(&candidates, consecutive as u64).expect("two candidates");
            if pick != (1, 1) {
                break;
            }
            consecutive += 1;
            // The player keeps losing: competence falls 0.1 per expedition until the floor.
            level = (level - 0.1).max(0.0);
            if let Some(h) = d.cells.get_mut(&key((1, 1))) {
                h.observe(level);
            }
            assert!(consecutive < 100, "the director NEVER leaves a declining cell — worse than H-4 feared");
        }
        // Measured 2026-07-31: 10 consecutive returns while the decline runs 0.5 → 0.0 and the
        // window drains to all-zeros (interest stays ≥ 0.1 the whole way down), then the first
        // non-(1,1) pick comes only from the 50/50 tie once BOTH cells sit at interest 0. So the
        // director rides a losing streak all the way to the floor — never longer (the escape is
        // real), and never shorter (there is no early mercy).
        assert!(
            (10..=30).contains(&consecutive),
            "expected the full ride down plus at most a few ties, got {consecutive}"
        );
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
    tally: Option<Res<crate::site::ExpeditionTally>>,
    survivors: Query<(), With<crate::squad::Unit>>,
    contained: Query<(), With<crate::containment::Contained>>,
    secured: Option<Res<crate::containment::SiteSecured>>,
    outcome: Option<Res<crate::session::RunOutcome>>,
) {
    let Some(tally) = tally else { return };
    let report = ExpeditionReport {
        squad_size: tally.squad_size,
        survivors: survivors.iter().count() as u32,
        captures: contained.iter().count() as u32,
        extracted: outcome.is_some_and(|o| matches!(*o, crate::session::RunOutcome::Victory)),
        // Derived exactly as `site::review::file_expedition_report` derives it, rather than hardcoded
        // to 0. A hardcoded zero made an expedition that left every nest uncapped score as competently
        // as one that capped them all — so the curriculum learned from a signal that disagreed with the
        // game's own definition of how the run went. `SiteSecured` may legitimately be absent in a
        // world with no nests, which is zero breaches rather than unknown.
        breaches: secured.map(|s| s.total.saturating_sub(s.capped) as u32).unwrap_or(0),
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
                // BEFORE `RunBuild::Config`, not before `World`: the dial writes `GameConfig`, and
                // `Config` is the stage where each consumer re-reads it. Ordering only against `World`
                // was correct-looking and inert — the consumers had already snapshotted at plugin build
                // and never looked at `GameConfig` again (FVS-H-8).
                pick_next_challenge.before(crate::session::RunBuild::Config),
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
