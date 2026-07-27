//! **The O5 performance review and the requisition budget** (FVS-P-1, FVS-P-2).
//!
//! After each expedition the Council rates the Director and issues an allowance. The player is not a
//! looter picking up coins; they are being **assessed**, which is why losing an operative is not just a
//! tactical loss but a line in a review.
//!
//! ## One source of truth for "how did that expedition go"
//!
//! [`ExpeditionReport`] reads the **same terms** `squad_ai::surprise::EpisodeOutcome` reports to the
//! offline search — survivors, captures, extraction, breaches, duration. That is deliberate and it is
//! the point of doing P-1 after E-4: the search and the Council must not be able to disagree about
//! whether a run went well, or the player is being graded on something the game is not optimising for.
//!
//! ## The budget floor — a decision the backlog left open, made here
//!
//! A performance-rated allowance can death-spiral: a bad run yields a small budget, which causes a
//! worse run. The floor is set at **the price of one capture device**, and the reasoning is that the
//! floor's job is not generosity — it is to guarantee the *loop remains attemptable*. A Director who
//! can still contain something can still recover; one who cannot is in a state the game has no way out
//! of, which is a design failure rather than a difficulty.
//!
//! **There is deliberately no "relieved of command" outcome.** A review that can end the campaign would
//! be a second lose condition competing with the squad wipe, and a strictly worse one: it fires from
//! accumulated mediocrity rather than from anything the player can see happening. [`Rating::Displeased`]
//! is the bottom, and it says so in as many words — the design doc's "displeased but you are not
//! relieved of command" band, made literal.
//!
//! ## Not evolvable
//!
//! The rating curve sits outside `WorldConfig` for the same reason `session::SessionConfig` does: it
//! defines what *performing well* means. A search free to retune it would move the measuring stick.

use serde::{Deserialize, Serialize};

/// What the Council saw. Derived from the same terms the QD fitness computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExpeditionReport {
    pub squad_size: u32,
    pub survivors: u32,
    /// Anomalies driven to `Contained` this expedition.
    pub captures: u32,
    /// Did the squad reach the extraction point?
    pub extracted: bool,
    /// Nest breaches / uncapped sources left behind.
    pub breaches: u32,
}

/// The Council's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rating {
    /// Contained and extracted with the squad intact.
    Exemplary,
    /// The objective was met.
    Satisfactory,
    /// It was not. **The bottom** — there is no rating below this, on purpose.
    Displeased,
}

impl Rating {
    /// What the Council actually says. Phrased so `Displeased` cannot be mistaken for a dismissal.
    pub fn remark(self) -> &'static str {
        match self {
            Rating::Exemplary => "EXEMPLARY. THE COUNCIL NOTES YOUR RESULTS.",
            Rating::Satisfactory => "SATISFACTORY. CONTINUE.",
            Rating::Displeased => "THE COUNCIL IS DISPLEASED. YOU ARE NOT RELIEVED OF COMMAND.",
        }
    }
}

/// Price list. **Consumables only** (FVS-P-2) — capabilities come from research, never from budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Consumable {
    CaptureDevice,
    QuarantineCharge,
    Medkit,
}

impl Consumable {
    pub const ALL: [Consumable; 3] =
        [Consumable::CaptureDevice, Consumable::QuarantineCharge, Consumable::Medkit];

    pub const fn price(self) -> u32 {
        match self {
            Consumable::CaptureDevice => 30,
            Consumable::QuarantineCharge => 50,
            Consumable::Medkit => 20,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Consumable::CaptureDevice => "CAPTURE DEVICE",
            Consumable::QuarantineCharge => "QUARANTINE CHARGE",
            Consumable::Medkit => "MEDKIT",
        }
    }
}

/// The floor: always enough to attempt one capture. See the module note on why this is the number.
pub const BUDGET_FLOOR: u32 = Consumable::CaptureDevice.price();

/// Rate an expedition.
///
/// Extraction is the hinge, because it is the hinge of the win condition too: a capture you could not
/// walk out with is not a secure, and the review must not say otherwise.
pub fn rate(r: &ExpeditionReport) -> Rating {
    if !r.extracted || r.captures == 0 {
        return Rating::Displeased;
    }
    if r.survivors == r.squad_size && r.breaches == 0 {
        return Rating::Exemplary;
    }
    Rating::Satisfactory
}

/// The allowance a rating earns, floored.
pub fn allowance(r: &ExpeditionReport) -> u32 {
    let base: u32 = match rate(r) {
        Rating::Exemplary => 140,
        Rating::Satisfactory => 90,
        Rating::Displeased => 0,
    };
    // Per-capture bonus, so a Director who contains two anomalies is funded better than one who
    // contained the minimum — the review rewards the pivot rather than mere survival.
    let yield_bonus = r.captures.saturating_mul(25);
    // Losing operatives costs, and it costs per head rather than as a cliff, so a four-of-five run is
    // meaningfully better than a one-of-five run rather than both reading as "not exemplary".
    let losses = r.squad_size.saturating_sub(r.survivors);
    let penalty = losses.saturating_mul(20);
    base.saturating_add(yield_bonus).saturating_sub(penalty).max(BUDGET_FLOOR)
}

/// The Director's standing and unspent funds. Meta-progress: not run-scoped.
///
/// **Serialized** (FVS-P-3's *Done when*: "the budget round-trips through save/load"). Meta-progress
/// that does not survive a restart is not meta-progress — an allowance earned by an exemplary
/// expedition and then lost to quitting makes the review a per-session score rather than a campaign.
#[derive(
    bevy::prelude::Resource, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct O5Standing {
    pub budget: u32,
    pub last_rating: Option<Rating>,
    pub expeditions: u32,
}

impl O5Standing {
    /// Fold in a finished expedition.
    pub fn record(&mut self, report: &ExpeditionReport) {
        self.last_rating = Some(rate(report));
        self.budget = self.budget.saturating_add(allowance(report));
        self.expeditions += 1;
    }

    /// Spend, if affordable. Returns whether the purchase happened — one path, no partial buys.
    pub fn buy(&mut self, item: Consumable) -> bool {
        let p = item.price();
        if self.budget < p {
            return false;
        }
        self.budget -= p;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perfect() -> ExpeditionReport {
        ExpeditionReport { squad_size: 5, survivors: 5, captures: 1, extracted: true, breaches: 0 }
    }

    #[test]
    fn extraction_is_the_hinge_of_the_review_as_well_as_the_win() {
        // A capture you could not walk out with is not a secure. The review must not disagree with the
        // win condition about that, or the player is graded on something the game does not reward.
        let mut r = perfect();
        r.extracted = false;
        assert_eq!(rate(&r), Rating::Displeased);
        r = perfect();
        r.captures = 0;
        assert_eq!(rate(&r), Rating::Displeased, "surviving without containing anything is not the job");
    }

    #[test]
    fn the_budget_can_never_fall_below_one_capture_device() {
        // THE anti-death-spiral property. The floor's job is not generosity — it is that the loop stays
        // attemptable. A Director who cannot afford to contain anything is in a state with no way out.
        let disaster = ExpeditionReport {
            squad_size: 5,
            survivors: 0,
            captures: 0,
            extracted: false,
            breaches: 9,
        };
        assert_eq!(rate(&disaster), Rating::Displeased);
        assert!(
            allowance(&disaster) >= Consumable::CaptureDevice.price(),
            "the floor must always fund at least one capture attempt"
        );
    }

    #[test]
    fn a_worse_expedition_never_pays_better_than_a_better_one() {
        // Monotonicity, which is easy to break with a bonus/penalty scheme and reads as the review being
        // arbitrary. Losses only ever cost; captures only ever pay.
        let good = perfect();
        let mut fewer_survivors = perfect();
        fewer_survivors.survivors = 3;
        let mut no_captures = perfect();
        no_captures.captures = 0;
        assert!(allowance(&good) >= allowance(&fewer_survivors));
        assert!(allowance(&good) >= allowance(&no_captures));
    }

    #[test]
    fn containing_more_is_funded_better_than_containing_the_minimum() {
        let one = perfect();
        let mut three = perfect();
        three.captures = 3;
        assert!(allowance(&three) > allowance(&one), "the review must reward the pivot, not mere survival");
    }

    #[test]
    fn the_bottom_rating_explicitly_is_not_a_dismissal() {
        // A review that could end the campaign would be a second lose condition, firing from
        // accumulated mediocrity rather than from anything the player can see. The copy has to say so.
        let remark = Rating::Displeased.remark();
        assert!(remark.contains("NOT RELIEVED"), "the bottom band must be explicit: {remark}");
    }

    #[test]
    fn budget_buys_consumables_and_a_failed_purchase_changes_nothing() {
        let mut s = O5Standing { budget: 40, ..Default::default() };
        assert!(s.buy(Consumable::CaptureDevice));
        assert_eq!(s.budget, 10);
        assert!(!s.buy(Consumable::CaptureDevice), "cannot afford it");
        assert_eq!(s.budget, 10, "a refused purchase must not partially spend");
    }

    #[test]
    fn nothing_purchasable_is_a_capability() {
        // FVS-P-2's hard rule, enforced rather than remembered: budget buys CONSUMABLES, never
        // capabilities — those come from research (F-2). Keeping the two economies disjoint BY KIND is
        // what stops the soft currency eating the research loop. If a `Consumable` ever names something
        // from `research::Capability`, this fails.
        let caps: Vec<&str> = crate::research::Capability::ALL.iter().map(|c| c.label()).collect();
        for item in Consumable::ALL {
            assert!(
                !caps.contains(&item.label()),
                "{:?} sells a capability; budget must buy consumables only",
                item
            );
            assert!(item.price() > 0, "{item:?} must cost something");
        }
    }

    #[test]
    fn standing_accumulates_across_expeditions() {
        let mut s = O5Standing::default();
        s.record(&perfect());
        let after_one = s.budget;
        s.record(&perfect());
        assert!(s.budget > after_one, "budget accrues; it is meta-progress");
        assert_eq!(s.expeditions, 2);
        assert_eq!(s.last_rating, Some(Rating::Exemplary));
    }
}
