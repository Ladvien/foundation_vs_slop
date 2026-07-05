//! Utility decision layer — score competing behaviours at runtime and pick one (Dill, "Dual-Utility
//! Reasoning", Game AI Pro 2 Ch.3). A behaviour's score is the *product* of its considerations (each a
//! normalized input passed through a response curve); selection is **dual-utility**: take the highest
//! occupied rank bucket, then weight-based-random within it (variety without dumb low-utility picks).
//!
//! Extensibility (Colledanchise & Ögren's standard-interface building block): a behaviour is a small
//! data literal — `Mode` + `rank` + a list of `Consideration { Input, Curve }` + a `TargetKind`.
//! Adding a behaviour or an input/curve is one literal; the engine never changes.

use bevy::math::Vec3;
use bevy::prelude::Component;

use super::drives::{DriveId, DRIVE_COUNT};
use super::field::FieldId;
use crate::util::rand01;

/// The executable outcome of a decision — read by the (wrapped) locomotion systems to select which
/// movement mechanic runs this tick.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Forage,
    Latch,
    Flee,
    Chase,
    Wander,
    HuntBlood,
    /// Seek out a meat gib to scavenge.
    SeekMeat,
    /// Haul a grabbed gib home to the nest.
    Carry,
}

/// A perception fact the brain reads (extend freely).
#[derive(Clone, Copy)]
pub enum Fact {
    /// Distance to the nearest unit (large when none).
    NearestUnitDist,
    /// Peak value of the SCENT field anywhere (global "is there a frenzy" signal).
    ScentHotspot,
    /// SCENT/THREAT sampled at the agent's own position.
    #[allow(dead_code)]
    ThreatHere,
    /// Agent health fraction `[0,1]`.
    SelfHealthFrac,
    /// Peak value of the MEAT field anywhere (is there a pile worth foraging).
    MeatHotspot,
    /// 1.0 while this crab is hauling a lifted gib (latches the Carry behaviour).
    CarryingMeat,
}

/// What a consideration reads. Extensible: a drive, a field sample at self, or a perception fact.
#[derive(Clone, Copy)]
pub enum Input {
    #[allow(dead_code)] // drive inputs (hunger/fear) land with the crab brain (Phase 4)
    Drive(DriveId),
    #[allow(dead_code)] // field-at-self inputs land with the crab brain (Phase 4)
    Field(FieldId),
    Perc(Fact),
}

/// Parametric response curve mapping a raw input to a `[0,1]` utility. Params are RON-tunable.
#[derive(Clone, Copy)]
pub enum Curve {
    /// `m*x + b`, clamped.
    Linear { m: f32, b: f32 },
    /// `k * x^exp`, clamped — sharp ramp / diminishing returns.
    #[allow(dead_code)]
    Power { k: f32, exp: f32 },
    /// Logistic `1/(1+e^-k(x-x0))` — a soft threshold (fear turning on). (Crab Flee, Phase 4.)
    #[allow(dead_code)]
    Logistic { k: f32, x0: f32 },
    /// Hard threshold: `x >= threshold ? above : below`.
    #[allow(dead_code)]
    Step { threshold: f32, below: f32, above: f32 },
}

impl Curve {
    pub fn eval(&self, x: f32) -> f32 {
        let y = match *self {
            Curve::Linear { m, b } => m * x + b,
            Curve::Power { k, exp } => k * x.max(0.0).powf(exp),
            Curve::Logistic { k, x0 } => 1.0 / (1.0 + (-k * (x - x0)).exp()),
            Curve::Step {
                threshold,
                below,
                above,
            } => {
                if x >= threshold {
                    above
                } else {
                    below
                }
            }
        };
        y.clamp(0.0, 1.0)
    }
}

/// One scoring factor: read an input, shape it with a curve.
pub struct Consideration {
    pub input: Input,
    pub curve: Curve,
}

/// Where a chosen behaviour aims (resolved from perception when the behaviour is selected).
#[derive(Clone, Copy)]
pub enum TargetKind {
    None,
    NearestUnit,
    ScentHotspot,
    /// The peak of the MEAT field (coarse aim; the exact gib is a per-crab entity link).
    MeatHotspot,
    /// The crab's home nest (Carry destination; resolved from the carried gib, not from `decide`).
    Nest,
}

/// A complete behaviour: a small data literal.
pub struct Behavior {
    pub mode: Mode,
    /// Dual-utility priority bucket — a higher rank with any positive score wins outright over lower.
    pub rank: u8,
    pub considerations: Vec<Consideration>,
    pub target: TargetKind,
}

impl Behavior {
    /// Product-of-considerations score (Dill). Empty considerations → 1.0 (unconditional).
    fn score(&self, perc: &Perception) -> f32 {
        let mut s = 1.0;
        for c in &self.considerations {
            s *= c.curve.eval(perc.read(c.input));
            if s <= 0.0 {
                return 0.0;
            }
        }
        s
    }
}

/// Everything a decision reads about one agent + the world, built once per think tick. Some fields are
/// consumed by the crab brain / steering in Phase 4 (a decision reads only what its behaviours need).
#[allow(dead_code)]
pub struct Perception {
    pub pos: Vec3,
    pub nearest_unit: Option<Vec3>,
    pub nearest_dist: f32,
    pub health_frac: f32,
    pub drives: [f32; DRIVE_COUNT],
    pub scent_hotspot: Vec3,
    pub scent_val: f32,
    pub threat_here: f32,
    pub meat_hotspot: Vec3,
    pub meat_val: f32,
    /// 1.0 while this crab is hauling a lifted gib.
    pub carrying: f32,
}

impl Perception {
    fn read(&self, input: Input) -> f32 {
        match input {
            Input::Drive(id) => self.drives[id.0],
            Input::Field(_) => self.threat_here, // only THREAT-at-self is used so far
            Input::Perc(Fact::NearestUnitDist) => self.nearest_dist,
            Input::Perc(Fact::ScentHotspot) => self.scent_val,
            Input::Perc(Fact::ThreatHere) => self.threat_here,
            Input::Perc(Fact::SelfHealthFrac) => self.health_frac,
            Input::Perc(Fact::MeatHotspot) => self.meat_val,
            Input::Perc(Fact::CarryingMeat) => self.carrying,
        }
    }
}

/// A behaviour must score at least this to "turn on" and claim its rank (screens out curve tails).
const MIN_SCORE: f32 = 0.1;

/// Dual-utility selection (Dill): the highest rank bucket with any positive score, then weight-based
/// random within it. Returns the index of the chosen behaviour. `behaviors` must be non-empty and
/// should include an unconditional low-rank default (e.g. Wander) so a choice always exists.
pub fn decide(behaviors: &[Behavior], perc: &Perception, rng: &mut u32) -> usize {
    let scores: Vec<f32> = behaviors.iter().map(|b| b.score(perc)).collect();
    // Highest rank among behaviours whose score clears MIN_SCORE. The threshold (not just >0) is
    // essential: without it a high-rank behaviour's near-zero curve tail (e.g. a Logistic at ~0.01)
    // would still claim its rank and dominate. Dill: screen out low-weight options.
    let max_rank = behaviors
        .iter()
        .zip(&scores)
        .filter(|(_, s)| **s >= MIN_SCORE)
        .map(|(b, _)| b.rank)
        .max();
    let Some(max_rank) = max_rank else {
        return 0; // nothing scored — caller's first behaviour is the safety default
    };
    let total: f32 = behaviors
        .iter()
        .zip(&scores)
        .filter(|(b, _)| b.rank == max_rank)
        .map(|(_, s)| *s)
        .sum();
    let mut r = rand01(rng) * total;
    let mut last = 0;
    for (i, (b, s)) in behaviors.iter().zip(&scores).enumerate() {
        if b.rank != max_rank {
            continue;
        }
        last = i;
        r -= *s;
        if r <= 0.0 {
            return i;
        }
    }
    last
}
