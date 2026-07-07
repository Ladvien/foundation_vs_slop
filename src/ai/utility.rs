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
    /// Scout roam: range far and fast across floor + walls, hunting for prey.
    Scout,
    /// A scout that spotted prey tracks it, laying the vectorial rally pheromone toward its live position.
    Mark,
    /// Mass on the local rally pheromone (the scout-marked prey) — the swarm's recruited attack.
    Rally,
    /// Muster: a nearby crab was just wounded — converge on the squad and press (a retaliatory surge,
    /// driven by the local ALARM pheromone). The twin of Rally, but recruited by kin damage, not a scout.
    Muster,
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
    /// 1.0 while this scout is holding a live prey sighting (latches Mark, so it tracks + marks the prey).
    PreySpotted,
    /// Magnitude of the vectorial rally pheromone sampled at the agent's OWN cell — a *local* read (not a
    /// global peak), so only crabs actually near a scout-marked sighting rally / suppress their flight.
    RallyHere,
    /// ALARM field sampled at the agent's own cell — a *local* read of the "wounded kin" warning cry, so
    /// only crabs within ~one room of a casualty muster (gates Muster on) and press through fire (gates
    /// Flee off). Fades as the alarm evaporates, so fear resumes once the retaliation window closes.
    AlarmHere,
    /// 1.0 while the agent's own cell is in the squad's *live* line of sight (fog-of-war). The boss reads
    /// this so it pursues whenever it is visible AT ANY RANGE, not only inside the distance leash — a slow
    /// boss shot from across the room must still advance, not drift into Wander.
    SeenBySquad,
}

/// What a consideration reads. Extensible: a drive, a field sample at self, or a perception fact.
#[derive(Clone, Copy)]
pub enum Input {
    Drive(DriveId),
    #[allow(dead_code)] // no behaviour reads a raw field-at-self yet (drives cover it via TrackField)
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
    /// Logistic `1/(1+e^-k(x-x0))` — a soft threshold (fear turning on).
    Logistic { k: f32, x0: f32 },
    /// Hard threshold: `x >= threshold ? above : below`.
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
    /// A marking scout's live prey sighting — it approaches this to keep the rally pheromone fresh
    /// (resolved from the scout's stored sighting, not from the field).
    TrackedPrey,
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
    /// 1.0 while this scout has a sighting to report (drives Report over roam).
    pub prey_spotted: f32,
    /// Magnitude of the vectorial rally pheromone at the agent's own cell (a local read — see
    /// [`Fact::RallyHere`]; gates Rally on and Flee off only for crabs actually near a marked sighting).
    pub rally_val: f32,
    /// ALARM field at the agent's own cell (a local read — see [`Fact::AlarmHere`]; gates Muster on and
    /// Flee off only for crabs within ~one room of a wounded crab).
    pub alarm_val: f32,
    /// 1.0 while this agent's cell is in the squad's live LOS (see [`Fact::SeenBySquad`]); the boss's
    /// "pursue whenever seen, at any range" aggro term.
    pub seen_by_squad: f32,
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
            Input::Perc(Fact::PreySpotted) => self.prey_spotted,
            Input::Perc(Fact::RallyHere) => self.rally_val,
            Input::Perc(Fact::AlarmHere) => self.alarm_val,
            Input::Perc(Fact::SeenBySquad) => self.seen_by_squad,
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
    // Weight only the behaviours that BOTH sit in the winning bucket AND clear MIN_SCORE. Re-applying
    // the screen here (not just when picking max_rank) matters when a strong behaviour lifts a shared
    // rank: a sub-threshold sibling in that bucket (e.g. a faint SeekMeat trace beside a 0.9 Latch)
    // must not sneak into the weighted-random draw. Same predicate in the sum and the pick loop.
    let eligible = |b: &Behavior, s: f32| b.rank == max_rank && s >= MIN_SCORE;
    let total: f32 = behaviors
        .iter()
        .zip(&scores)
        .filter(|(b, s)| eligible(b, **s))
        .map(|(_, s)| *s)
        .sum();
    let mut r = rand01(rng) * total;
    let mut last = 0;
    for (i, (b, s)) in behaviors.iter().zip(&scores).enumerate() {
        if !eligible(b, *s) {
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

#[cfg(test)]
mod tests {
    // Pure decision logic — no App, no ECS (mirrors the seed-in/assert-out convention in
    // `wfc.rs`/`autogib.rs`). Locks the response curves and the dual-utility bucket + weighted-random
    // selection so a silent change to either is caught (Dill, "Dual-Utility Reasoning").
    use super::*;

    /// A neutral perception; each test overrides only the fields its behaviours read.
    fn zeroed() -> Perception {
        Perception {
            pos: Vec3::ZERO,
            nearest_unit: None,
            nearest_dist: 0.0,
            health_frac: 0.0,
            drives: [0.0; DRIVE_COUNT],
            scent_hotspot: Vec3::ZERO,
            scent_val: 0.0,
            threat_here: 0.0,
            meat_hotspot: Vec3::ZERO,
            meat_val: 0.0,
            carrying: 0.0,
            prey_spotted: 0.0,
            rally_val: 0.0,
            alarm_val: 0.0,
            seen_by_squad: 0.0,
        }
    }

    fn behavior(mode: Mode, rank: u8, considerations: Vec<Consideration>) -> Behavior {
        Behavior { mode, rank, considerations, target: TargetKind::None }
    }

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1.0e-5, "expected {b}, got {a}");
    }

    #[test]
    fn curve_linear_clamps_to_unit_range() {
        let c = Curve::Linear { m: 1.0, b: 0.0 };
        approx(c.eval(0.5), 0.5);
        approx(c.eval(2.0), 1.0); // clamped high
        approx(c.eval(-1.0), 0.0); // clamped low
    }

    #[test]
    fn curve_power_and_logistic_and_step() {
        approx(Curve::Power { k: 1.0, exp: 2.0 }.eval(0.5), 0.25);
        // Logistic is exactly 0.5 at its midpoint x0, for any slope k.
        approx(Curve::Logistic { k: 10.0, x0: 0.5 }.eval(0.5), 0.5);
        let step = Curve::Step { threshold: 0.5, below: 0.0, above: 1.0 };
        approx(step.eval(0.499), 0.0);
        approx(step.eval(0.5), 1.0); // boundary is inclusive (>=)
    }

    #[test]
    fn higher_rank_bucket_wins_outright() {
        // b0 = unconditional Wander (rank 0, score 1.0); b1 = Flee (rank 2) scoring health_frac.
        let behaviors = vec![
            behavior(Mode::Wander, 0, vec![]),
            behavior(
                Mode::Flee,
                2,
                vec![Consideration {
                    input: Input::Perc(Fact::SelfHealthFrac),
                    curve: Curve::Linear { m: 1.0, b: 0.0 },
                }],
            ),
        ];
        let mut perc = zeroed();
        perc.health_frac = 0.9; // Flee scores 0.9 >= MIN_SCORE and outranks Wander
        // rng is irrelevant when the winning bucket has a single member.
        let mut rng = 12345u32;
        assert_eq!(decide(&behaviors, &perc, &mut rng), 1);
    }

    #[test]
    fn sub_threshold_high_rank_is_screened_out() {
        // Flee's score (0.05) is below MIN_SCORE (0.1), so it must NOT claim its rank — the
        // unconditional low-rank default wins instead. This is the exact tail-screening Dill warns about.
        let behaviors = vec![
            behavior(Mode::Wander, 0, vec![]),
            behavior(
                Mode::Flee,
                2,
                vec![Consideration {
                    input: Input::Perc(Fact::SelfHealthFrac),
                    curve: Curve::Linear { m: 1.0, b: 0.0 },
                }],
            ),
        ];
        let mut perc = zeroed();
        perc.health_frac = 0.05;
        let mut rng = 1u32;
        assert_eq!(decide(&behaviors, &perc, &mut rng), 0);
    }

    #[test]
    fn nothing_scoring_returns_safety_default() {
        // Single behaviour whose consideration reads 0 → screened out entirely → decide falls back to
        // index 0 (the documented "caller's first behaviour is the safety default").
        let behaviors = vec![behavior(
            Mode::Flee,
            2,
            vec![Consideration {
                input: Input::Perc(Fact::SelfHealthFrac),
                curve: Curve::Linear { m: 1.0, b: 0.0 },
            }],
        )];
        let perc = zeroed(); // health_frac = 0.0
        let mut rng = 7u32;
        assert_eq!(decide(&behaviors, &perc, &mut rng), 0);
    }

    #[test]
    fn weighted_random_within_bucket_tracks_scores() {
        // Two same-rank behaviours scoring 0.9 and 0.1 → over many draws the 0.9 option is picked ~90%.
        // Threads one LCG state across calls (the production pattern: a per-agent `rng` field).
        let behaviors = vec![
            behavior(
                Mode::Chase,
                1,
                vec![Consideration {
                    input: Input::Perc(Fact::ScentHotspot),
                    curve: Curve::Linear { m: 1.0, b: 0.0 },
                }],
            ),
            behavior(
                Mode::SeekMeat,
                1,
                vec![Consideration {
                    input: Input::Perc(Fact::MeatHotspot),
                    curve: Curve::Linear { m: 1.0, b: 0.0 },
                }],
            ),
        ];
        let mut perc = zeroed();
        perc.scent_val = 0.9;
        perc.meat_val = 0.1;
        let mut rng = 0u32;
        let n = 5000;
        let chase = (0..n).filter(|_| decide(&behaviors, &perc, &mut rng) == 0).count();
        let frac = chase as f32 / n as f32;
        assert!((frac - 0.9).abs() < 0.05, "expected ~0.9 Chase share, got {frac}");
    }

    #[test]
    fn decide_is_deterministic_for_a_seed() {
        let behaviors = vec![
            behavior(Mode::Wander, 0, vec![]),
            behavior(
                Mode::Chase,
                1,
                vec![Consideration {
                    input: Input::Perc(Fact::ScentHotspot),
                    curve: Curve::Linear { m: 1.0, b: 0.0 },
                }],
            ),
        ];
        let mut perc = zeroed();
        perc.scent_val = 0.5;
        let a = {
            let mut r = 999u32;
            decide(&behaviors, &perc, &mut r)
        };
        let b = {
            let mut r = 999u32;
            decide(&behaviors, &perc, &mut r)
        };
        assert_eq!(a, b);
    }
}
