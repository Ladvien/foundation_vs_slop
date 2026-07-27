//! **The containment rule model — what it takes to hold an anomaly in its containable basin.**
//!
//! A rule is pure data: a set of [`FieldCondition`]s over the stigmergy channels in
//! [`crate::ai::field`], a hold duration, and what a mid-hold failure does. It is authored in RON and
//! evaluated by the containment system (FVS-B-3); nothing here spawns, mutates, or schedules anything,
//! which is why the whole module is unit-testable without an `App`.
//!
//! # Why conditions over *fields*, not over the anomaly
//!
//! Containment is "drive its drives/fields into a basin and hold", not HP depletion — so the predicate
//! has to read the same shared medium the creatures already coordinate through. Holland & Melhuish
//! (1999, *Stigmergy, self-organization, and sorting in collective robotics*, DOI
//! 10.1162/106454699568737, §1) enumerate exactly three ways a trace in the environment can affect an
//! agent, and name the mechanism this model uses:
//!
//! > "The qualitative effect in Method 1 may of course be internally controlled by some **threshold
//! > mechanism acting on a quantitatively varying input**."
//!
//! That is this type: a scalar channel sampled at a cell, compared against a threshold. Reusing the
//! stigmergy substrate rather than inventing a parallel one means every existing depositor — gunfire,
//! gaze, dread, noise — is already a containment *tool*, and a new channel becomes one for free.
//!
//! # `sign` is not a convenience
//!
//! Two anomalies can read the same channel with opposite polarity, and the roster already depends on
//! it: `ATTENTION` is documented in `ai::field` as being read "with **opposite signs** — the mould
//! recoils from it … while a marked predator is *drawn* to it". So SCP-1048 is contained by keeping
//! attention **above** a threshold (out-watch it, FVS-C-3) while another anomaly is contained by
//! keeping it **below** one. [`Sign`] carries that polarity in the data instead of forking the
//! evaluator, so there is exactly one code path for both.

use serde::{Deserialize, Serialize};

use crate::ai::field::{FieldId, CHANNEL_COUNT};

/// Which side of the threshold satisfies a condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Sign {
    /// Satisfied while the channel reads **at or above** the threshold (drive it up: flood the cell
    /// with attention, noise, light).
    AtLeast,
    /// Satisfied while the channel reads **at or below** the threshold (starve it: break line of
    /// sight, stop shooting, let the dread evaporate).
    AtMost,
}

/// One clause of a rule: a channel, a polarity, and a level.
///
/// Comparisons are inclusive (`>=` / `<=`) so a threshold of `0.0` with [`Sign::AtMost`] means "no
/// trace at all" rather than an unsatisfiable strict inequality.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldCondition {
    /// Index into the stigmergy channels (see [`crate::ai::field::FieldId`]).
    pub channel: usize,
    /// Which side of `threshold` satisfies this clause.
    pub sign: Sign,
    /// The level to compare against.
    pub threshold: f32,
}

impl FieldCondition {
    /// Is this clause satisfied by `value`, the channel's reading at the anomaly's cell?
    ///
    /// NaN is **not** satisfied under either sign: both comparisons are false for NaN, which is the
    /// behaviour we want — a corrupt field reading must not silently complete a capture. It cannot
    /// arise from the shipped field code (deposits and evaporation are finite), and this is the one
    /// place the property is cheap to guarantee rather than assert.
    pub fn is_met(&self, value: f32) -> bool {
        match self.sign {
            Sign::AtLeast => value >= self.threshold,
            Sign::AtMost => value <= self.threshold,
        }
    }

    /// The channel as a [`FieldId`], or `None` if it is out of range.
    pub fn field(&self) -> Option<FieldId> {
        (self.channel < CHANNEL_COUNT).then_some(FieldId(self.channel))
    }
}

/// What a mid-hold failure costs.
///
/// Two policies, and they are genuinely different mechanics rather than a difficulty knob: `Reset`
/// makes containment a *sustained* task (one slip and you start the timer again — the tense one),
/// `Keep` makes it *cumulative* (progress banks, so a long fight can be won in pieces). Which one a
/// given anomaly uses is part of its identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum OnBreak {
    /// Lose all accumulated hold.
    Reset,
    /// Keep the hold accumulated so far and continue when the conditions are met again.
    Keep,
}

/// The full containment rule for one anomaly.
///
/// **Every** condition must hold simultaneously (conjunction). Disjunction is deliberately absent: an
/// "either of these" rule reads to the player as two different containment procedures, and the HUD
/// (FVS-L-1) has to explain *why* progress is happening — an OR would make that explanation ambiguous.
/// If an anomaly genuinely needs two routes, that is two rules and an explicit choice, not a hidden
/// branch inside one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainmentRule {
    /// All clauses, ANDed. An empty list is rejected by [`Self::validate`] — a rule satisfied by doing
    /// nothing would complete on its own timer.
    pub requires: Vec<FieldCondition>,
    /// How long the basin must be held, in seconds of simulated time.
    pub hold_secs: f32,
    /// What a mid-hold failure costs.
    pub break_on_fail: OnBreak,
}

impl ContainmentRule {
    /// Are all clauses satisfied by `sample(channel) -> value`?
    ///
    /// Takes a sampler rather than a field grid so the predicate is testable without a `Dungeon` or a
    /// `Stig`, and so the caller decides *where* to sample (the anomaly's cell today; an area average
    /// for the quarantine archetype in FVS-B-6) without this type knowing about geometry.
    pub fn is_satisfied(&self, mut sample: impl FnMut(FieldId) -> f32) -> bool {
        self.requires.iter().all(|c| match c.field() {
            Some(f) => c.is_met(sample(f)),
            // Unreachable for a validated rule; false rather than true so an unvalidated rule fails
            // closed (never completes) instead of capturing for free.
            None => false,
        })
    }

    /// Which clauses are currently unmet, as indices into [`Self::requires`].
    ///
    /// This is what the containment HUD (FVS-L-1) reads to tell the player *why* a capture is stalling
    /// — the item's acceptance is "players can read why containment is progressing/breaking", and a
    /// bare bool cannot answer it.
    pub fn unmet(&self, mut sample: impl FnMut(FieldId) -> f32) -> Vec<usize> {
        self.requires
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.field().is_some_and(|f| c.is_met(sample(f))))
            .map(|(i, _)| i)
            .collect()
    }

    /// Reject a malformed rule at load, loudly — one path, no fallback: a bad rule is a content bug and
    /// must fail at the door rather than produce an anomaly that captures itself or can never be caught.
    pub fn validate(&self) -> Result<(), String> {
        if self.requires.is_empty() {
            return Err("containment rule has no conditions — it would complete on its timer alone".into());
        }
        if !(self.hold_secs.is_finite() && self.hold_secs > 0.0) {
            return Err(format!("hold_secs must be finite and > 0, got {}", self.hold_secs));
        }
        for (i, c) in self.requires.iter().enumerate() {
            if c.field().is_none() {
                return Err(format!(
                    "condition {i} names channel {} but only 0..{CHANNEL_COUNT} exist",
                    c.channel
                ));
            }
            if !c.threshold.is_finite() {
                return Err(format!("condition {i} has a non-finite threshold {}", c.threshold));
            }
        }
        // A channel named twice with the same sign is a content mistake (one clause is dead), and with
        // opposite signs is either a band or a contradiction — both are worth stating explicitly rather
        // than silently ANDing.
        for (i, a) in self.requires.iter().enumerate() {
            for b in &self.requires[i + 1..] {
                if a.channel == b.channel && a.sign == b.sign {
                    return Err(format!(
                        "channel {} is constrained twice with the same sign — one clause is dead",
                        a.channel
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sampler over a fixed table, standing in for the field grid at one cell.
    fn sampler(values: [f32; CHANNEL_COUNT]) -> impl FnMut(FieldId) -> f32 {
        move |f: FieldId| values[f.0]
    }

    fn rule(requires: Vec<FieldCondition>) -> ContainmentRule {
        ContainmentRule { requires, hold_secs: 3.0, break_on_fail: OnBreak::Reset }
    }

    #[test]
    fn a_threshold_is_inclusive_on_both_signs() {
        let at_least = FieldCondition { channel: 0, sign: Sign::AtLeast, threshold: 0.5 };
        assert!(at_least.is_met(0.5), "at-least is inclusive");
        assert!(at_least.is_met(0.6));
        assert!(!at_least.is_met(0.4));

        let at_most = FieldCondition { channel: 0, sign: Sign::AtMost, threshold: 0.5 };
        assert!(at_most.is_met(0.5), "at-most is inclusive");
        assert!(at_most.is_met(0.4));
        assert!(!at_most.is_met(0.6));
    }

    #[test]
    fn a_zero_threshold_at_most_means_no_trace_at_all() {
        // The reason the comparisons are inclusive: `AtMost 0.0` must be satisfiable by an empty cell.
        let c = FieldCondition { channel: 0, sign: Sign::AtMost, threshold: 0.0 };
        assert!(c.is_met(0.0));
        assert!(!c.is_met(f32::MIN_POSITIVE));
    }

    #[test]
    fn a_corrupt_reading_never_satisfies_a_condition() {
        let at_least = FieldCondition { channel: 0, sign: Sign::AtLeast, threshold: 0.5 };
        let at_most = FieldCondition { channel: 0, sign: Sign::AtMost, threshold: 0.5 };
        assert!(!at_least.is_met(f32::NAN), "NaN must not complete a capture");
        assert!(!at_most.is_met(f32::NAN), "NaN must not complete a capture");
    }

    #[test]
    fn opposite_signs_on_one_channel_express_the_two_attention_poles() {
        // The 1048 pole: contained while the cell stays WATCHED (`ATTENTION` high).
        let out_watch = rule(vec![FieldCondition {
            channel: FieldId::ATTENTION.0,
            sign: Sign::AtLeast,
            threshold: 0.4,
        }]);
        // The mould pole: contained while the cell stays UNWATCHED.
        let unwatched = rule(vec![FieldCondition {
            channel: FieldId::ATTENTION.0,
            sign: Sign::AtMost,
            threshold: 0.1,
        }]);

        let mut watched = [0.0; CHANNEL_COUNT];
        watched[FieldId::ATTENTION.0] = 0.9;
        assert!(out_watch.is_satisfied(sampler(watched)));
        assert!(!unwatched.is_satisfied(sampler(watched)));

        let dark = [0.0; CHANNEL_COUNT];
        assert!(!out_watch.is_satisfied(sampler(dark)));
        assert!(unwatched.is_satisfied(sampler(dark)));
    }

    #[test]
    fn every_condition_must_hold_and_the_unmet_ones_are_reported() {
        let r = rule(vec![
            FieldCondition { channel: FieldId::ATTENTION.0, sign: Sign::AtLeast, threshold: 0.4 },
            FieldCondition { channel: FieldId::THREAT_GUN.0, sign: Sign::AtMost, threshold: 0.1 },
        ]);

        let mut half = [0.0; CHANNEL_COUNT];
        half[FieldId::ATTENTION.0] = 0.9;
        half[FieldId::THREAT_GUN.0] = 0.8; // still shooting — breaks the second clause
        assert!(!r.is_satisfied(sampler(half)), "a conjunction fails if any clause fails");
        assert_eq!(r.unmet(sampler(half)), vec![1], "the HUD must be able to name the failing clause");

        let mut both = [0.0; CHANNEL_COUNT];
        both[FieldId::ATTENTION.0] = 0.9;
        assert!(r.is_satisfied(sampler(both)));
        assert!(r.unmet(sampler(both)).is_empty());
    }

    #[test]
    fn a_malformed_rule_is_rejected_at_the_door() {
        assert!(rule(vec![]).validate().is_err(), "an empty rule would capture on its timer alone");

        let bad_channel = rule(vec![FieldCondition {
            channel: CHANNEL_COUNT,
            sign: Sign::AtLeast,
            threshold: 0.1,
        }]);
        assert!(bad_channel.validate().is_err());
        // ...and it fails CLOSED if it somehow reaches evaluation unvalidated.
        assert!(!bad_channel.is_satisfied(sampler([1.0; CHANNEL_COUNT])));

        let mut zero_hold = rule(vec![FieldCondition {
            channel: 0,
            sign: Sign::AtLeast,
            threshold: 0.1,
        }]);
        zero_hold.hold_secs = 0.0;
        assert!(zero_hold.validate().is_err(), "a zero hold is an instant capture");

        let dup = rule(vec![
            FieldCondition { channel: 0, sign: Sign::AtLeast, threshold: 0.1 },
            FieldCondition { channel: 0, sign: Sign::AtLeast, threshold: 0.7 },
        ]);
        assert!(dup.validate().is_err(), "a duplicated channel+sign leaves one clause dead");

        // A genuine band (opposite signs on one channel) is legal.
        let band = rule(vec![
            FieldCondition { channel: 0, sign: Sign::AtLeast, threshold: 0.2 },
            FieldCondition { channel: 0, sign: Sign::AtMost, threshold: 0.8 },
        ]);
        assert!(band.validate().is_ok(), "a two-sided band is a legitimate rule");
    }

    #[test]
    fn a_rule_round_trips_through_ron() {
        // The authoring format is RON (`assets/config/config.ron`), so parsing is part of the contract.
        let r = rule(vec![FieldCondition {
            channel: FieldId::ATTENTION.0,
            sign: Sign::AtLeast,
            threshold: 0.4,
        }]);
        let text = ron::ser::to_string(&r).expect("serializes");
        let back: ContainmentRule = ron::from_str(&text).expect("parses");
        assert_eq!(back, r);
    }

    #[test]
    fn an_unknown_field_in_a_rule_is_a_loud_parse_error() {
        // `deny_unknown_fields` everywhere — the same "one path, no fallback" rule the rest of the
        // config surface follows (see `config::GameConfig`): a typo must not silently default.
        let bad = "(requires: [(channel: 9, sign: AtLeast, threshold: 0.4, typo: 1.0)], \
                    hold_secs: 3.0, break_on_fail: Reset)";
        assert!(ron::from_str::<ContainmentRule>(bad).is_err());
    }
}
