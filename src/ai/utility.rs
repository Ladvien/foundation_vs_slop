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
use serde::{Deserialize, Serialize};

use super::drives::{DriveId, DRIVE_COUNT};
use crate::util::rand01;

/// The executable outcome of a decision — read by the (wrapped) locomotion systems to select which
/// movement mechanic runs this tick. `Deserialize` so squad **role** repertoires are authored as data
/// in `assets/config/roles.ron` (Jacopin, "Optimizing Practical Planning for Game AI", Game AI Pro 2
/// Ch.13 — actions as text files); the crab/boss brains stay code literals.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Deserialize, Serialize)]
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

    // --- Squad role actions (units only). Each is one entry in the shared action vocabulary, so it
    // doubles as the discrete high-level action of an RL policy (Wu et al., "Hierarchical Macro Strategy
    // Model for MOBA Game AI", AAAI 2019 — the RL "option" level). Executed by `squad_ai::actions`.
    /// Researcher: study the nearest unexamined furniture / creature / corpse and narrate a finding.
    Examine,
    /// Gunman: hold position and priority-fire on threats (drives `AimTarget`; no advance).
    Overwatch,
    /// Gunman: advance to contact on the nearest threat.
    Engage,
    /// Gunman: lay suppressing fire toward a threat bearing.
    Suppress,
    /// Psionic: sense anomalies / threat bearings through walls (reveals perception, not movement).
    PsiScan,
    /// Psionic: commune with the watcher (read the boss's state/mood).
    Commune,
    /// Psionic: raise a local ward that steadies squad morale / damps fear.
    Ward,
    /// Medic: move to and heal the nearest wounded ally.
    TendWounded,
    /// Engineer: secure / breach the nearest door.
    SecureDoor,
    /// Engineer: deploy a sensor that extends the squad's line of sight.
    DeploySensor,
    /// Any role: return toward the squad anchor when it has strayed past the leash (the cohesion pull).
    Regroup,
    /// Any role: loosely follow the moving squad anchor (default in-formation drift).
    FollowAnchor,
}

impl Mode {
    /// Every mode, in discriminant order — the action alphabet of the offline behaviour search
    /// (`squad_ai::surprise` indexes mode distributions by `Mode::index`). Pinned by
    /// `mode_all_is_dense_and_in_discriminant_order`, so adding a variant without listing it here is a
    /// loud test failure rather than a silently truncated distribution.
    pub const ALL: [Mode; 24] = [
        Mode::Forage,
        Mode::Latch,
        Mode::Flee,
        Mode::Chase,
        Mode::Wander,
        Mode::HuntBlood,
        Mode::SeekMeat,
        Mode::Carry,
        Mode::Scout,
        Mode::Mark,
        Mode::Rally,
        Mode::Muster,
        Mode::Examine,
        Mode::Overwatch,
        Mode::Engage,
        Mode::Suppress,
        Mode::PsiScan,
        Mode::Commune,
        Mode::Ward,
        Mode::TendWounded,
        Mode::SecureDoor,
        Mode::DeploySensor,
        Mode::Regroup,
        Mode::FollowAnchor,
    ];

    /// Dense index into [`Mode::ALL`]. Every variant is fieldless, so the discriminant *is* the index.
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Width of the action alphabet — the size of every mode distribution in `squad_ai::surprise`.
pub const MODE_COUNT: usize = Mode::ALL.len();

/// A perception fact the brain reads (extend freely).
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum Fact {
    /// Distance to the nearest unit (large when none).
    NearestUnitDist,
    /// Peak value of the SCENT field anywhere (global "is there a frenzy" signal).
    ScentHotspot,
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

    // --- Squad-unit facts (built by `squad_ai::perception`). Neutral (0 / far) for crabs & the boss. ---
    /// Planar distance from this unit to the squad anchor (large when no anchor). Drives Regroup.
    AnchorDist,
    /// Distance to the nearest examinable (furniture / creature / corpse) the unit can see (large when none).
    NearestExaminableDist,
    /// 1.0 while an *unexamined* examinable is within the Researcher's study range (latches Examine).
    HasUnexaminedNearby,
    /// Distance to the nearest wounded ally (large when none) — the Medic's TendWounded target.
    NearestWoundedAllyDist,
    /// 1.0 while any ally's health is critical within support range (gates the Medic's response).
    AllyDownNearby,
    /// 1.0 while a hostile bearing is known to this unit (a live threat to Overwatch / Engage / Suppress).
    ThreatBearingKnown,
    /// 1.0 while psychic residue (an anomaly signature) is sensed nearby — the Psionic's PsiScan hook.
    AnomalyResidueNearby,
    /// 1.0 once the unit has strayed past the cohesion leash (hysteretic — see `SquadFields::past_leash`).
    /// Gates `Regroup`, which outranks role work, so a strayed unit comes home instead of chaining duties
    /// across the map.
    PastLeash,
}

/// What a consideration reads. Extensible: a drive, a field sample at self, or a perception fact.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum Input {
    Drive(DriveId),
    Perc(Fact),
}

/// Parametric response curve mapping a raw input to a `[0,1]` utility. Params are RON-tunable.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub enum Curve {
    /// `m*x + b`, clamped.
    Linear { m: f32, b: f32 },
    /// Logistic `1/(1+e^-k(x-x0))` — a soft threshold (fear turning on).
    Logistic { k: f32, x0: f32 },
    /// Hard threshold: `x >= threshold ? above : below`.
    Step { threshold: f32, below: f32, above: f32 },
}

impl Curve {
    pub fn eval(&self, x: f32) -> f32 {
        let y = match *self {
            Curve::Linear { m, b } => m * x + b,
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
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Consideration {
    pub input: Input,
    pub curve: Curve,
}

/// Where a chosen behaviour aims (resolved from perception when the behaviour is selected).
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
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
    // --- Squad-unit targets (resolved by `squad_ai::perception`/`squad_think`). ---
    /// The squad anchor position (Regroup / FollowAnchor destination).
    SquadAnchor,
    /// The nearest examinable entity the unit can see (Examine destination).
    NearestExaminable,
    /// The nearest wounded ally (Medic's TendWounded destination).
    NearestWoundedAlly,
    /// The nearest known hostile (Gunman's Engage destination / Psionic's Commune subject).
    TrackedThreat,
}

/// A complete behaviour: a small data literal.
#[derive(Clone, Debug, Deserialize, Serialize)]
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

    /// Squad-unit perception (built by `squad_ai::perception`). Crab/boss `think` splats one neutral
    /// value: `squad: SquadFields::neutral()`.
    pub squad: SquadFields,
}

/// The "nothing of this kind is in range" distance. Deliberately **far but finite**: these values are fed
/// straight into the distance-falloff curves, where `f32::MAX` / `INFINITY` would produce `NaN` once
/// scaled or subtracted, and `0.0` would falsely read as "touching". Any real in-range distance is orders
/// of magnitude below it.
pub const NO_TARGET_DIST: f32 = 999.0;

/// The squad-only slice of a [`Perception`], grouped so non-squad builders (crab/boss) splat one
/// neutral value instead of listing every unit field. A plain struct (not `Default`-derived) so the
/// sentinel distances ([`NO_TARGET_DIST`], not `0.0`) are explicit — a `0.0` distance would falsely fire
/// the distance-falloff curves.
pub struct SquadFields {
    /// Squad anchor position + planar distance to it (see [`Fact::AnchorDist`]); [`NO_TARGET_DIST`] when
    /// there is no anchor.
    pub anchor: Option<Vec3>,
    pub anchor_dist: f32,
    /// The nearest examinable entity's position + distance, and whether an *unexamined* one is in range.
    pub nearest_examinable: Option<Vec3>,
    pub examinable_dist: f32,
    pub has_unexamined: f32,
    /// The nearest wounded ally's position + distance (Medic), and whether an ally is critical in range.
    pub nearest_wounded_ally: Option<Vec3>,
    pub wounded_ally_dist: f32,
    pub ally_down: f32,
    /// The nearest known hostile's position, plus gate flags for threat-driven and psi behaviours.
    pub tracked_threat: Option<Vec3>,
    pub threat_bearing_known: f32,
    pub anomaly_residue: f32,
    /// 1.0 once the unit has strayed past the cohesion leash, and back to 0.0 only once it has returned
    /// well inside it. Already hysteretic when it arrives here (`squad_ai::PerceptionLatch`), so `Regroup`
    /// can gate on it with a plain `Step` without chattering at the leash boundary.
    pub past_leash: f32,
}

impl SquadFields {
    /// The no-squad-context base (crabs, the boss, and every test's starting point).
    pub fn neutral() -> Self {
        SquadFields {
            anchor: None,
            anchor_dist: NO_TARGET_DIST,
            nearest_examinable: None,
            examinable_dist: NO_TARGET_DIST,
            has_unexamined: 0.0,
            nearest_wounded_ally: None,
            wounded_ally_dist: NO_TARGET_DIST,
            ally_down: 0.0,
            tracked_threat: None,
            threat_bearing_known: 0.0,
            anomaly_residue: 0.0,
            past_leash: 0.0,
        }
    }
}

impl Perception {
    fn read(&self, input: Input) -> f32 {
        match input {
            Input::Drive(id) => self.drives[id.0],
            Input::Perc(Fact::NearestUnitDist) => self.nearest_dist,
            Input::Perc(Fact::ScentHotspot) => self.scent_val,
            Input::Perc(Fact::SelfHealthFrac) => self.health_frac,
            Input::Perc(Fact::MeatHotspot) => self.meat_val,
            Input::Perc(Fact::CarryingMeat) => self.carrying,
            Input::Perc(Fact::PreySpotted) => self.prey_spotted,
            Input::Perc(Fact::RallyHere) => self.rally_val,
            Input::Perc(Fact::AlarmHere) => self.alarm_val,
            Input::Perc(Fact::SeenBySquad) => self.seen_by_squad,
            Input::Perc(Fact::AnchorDist) => self.squad.anchor_dist,
            Input::Perc(Fact::NearestExaminableDist) => self.squad.examinable_dist,
            Input::Perc(Fact::HasUnexaminedNearby) => self.squad.has_unexamined,
            Input::Perc(Fact::NearestWoundedAllyDist) => self.squad.wounded_ally_dist,
            Input::Perc(Fact::AllyDownNearby) => self.squad.ally_down,
            Input::Perc(Fact::ThreatBearingKnown) => self.squad.threat_bearing_known,
            Input::Perc(Fact::AnomalyResidueNearby) => self.squad.anomaly_residue,
            Input::Perc(Fact::PastLeash) => self.squad.past_leash,
        }
    }
}

/// A behaviour must score at least this to "turn on" and claim its rank (screens out curve tails).
const MIN_SCORE: f32 = 0.1;

/// A lower bound on this curve's output over the whole input domain.
///
/// Every [`Perception::read`] input is **non-negative** (drives and `health_frac` are clamped to `[0,1]`;
/// field samples, flags, and distances are all `>= 0`), so the domain is `x >= 0`. A bound of `0.0` means
/// "the world can switch this off" — sound, if sometimes pessimistic.
fn guaranteed_floor(curve: Curve) -> f32 {
    match curve {
        // Non-decreasing in x, so the floor sits at x = 0. A negative slope is unbounded below over the
        // half-line (distances have no ceiling), so it guarantees nothing.
        Curve::Linear { m, b } if m >= 0.0 => b.clamp(0.0, 1.0),
        Curve::Linear { .. } => 0.0,
        // Same argument: a positive k is increasing, so x = 0 is the floor; a negative k decays to 0.
        Curve::Logistic { k, x0 } if k >= 0.0 => Curve::Logistic { k, x0 }.eval(0.0),
        Curve::Logistic { .. } => 0.0,
        // A step only ever emits one of its two arms.
        Curve::Step { below, above, .. } => below.clamp(0.0, 1.0).min(above.clamp(0.0, 1.0)),
    }
}

/// A lower bound on the score this behaviour reaches no matter what the agent perceives. An empty
/// consideration list is unconditional at 1.0, matching [`Behavior::score`].
fn guaranteed_score(behavior: &Behavior) -> f32 {
    behavior.considerations.iter().map(|c| guaranteed_floor(c.curve)).product()
}

/// Startup guard: a repertoire must contain at least one **unconditional** behaviour that clears
/// [`MIN_SCORE`], so [`decide`] always finds a non-empty rank bucket.
///
/// This is what makes `decide`'s "nothing scored" branch unreachable for every shipped brain. Without it
/// that branch silently returns behaviour 0 — which for a squad role brain is the rank-4 *duty*
/// (Examine / TendWounded / SecureDoor), so a malformed repertoire would quietly make units perform their
/// duty instead of standing down. We refuse to ship that ambiguity: it is a loud startup failure, checked
/// once, rather than a silent per-tick fallback (the project's one-path rule).
pub fn validate_unconditional_default(behaviors: &[Behavior], who: &str) -> Result<(), String> {
    if behaviors.is_empty() {
        return Err(format!("{who}: empty behaviour repertoire"));
    }
    let best = behaviors.iter().map(guaranteed_score).fold(0.0f32, f32::max);
    if best >= MIN_SCORE {
        return Ok(());
    }
    Err(format!(
        "{who}: no unconditional behaviour scores >= MIN_SCORE ({MIN_SCORE}); every option can be gated \
         off by perception, so `decide` would have nothing to choose"
    ))
}

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
        // Unreachable for any shipped repertoire: `validate_unconditional_default` runs at startup and
        // rejects a brain in which every behaviour can be gated off, so some behaviour always clears
        // MIN_SCORE. Index 0 is the arbitrary-but-total answer if that invariant is ever broken; it is
        // never a *silent fallback*, because the brain could not have loaded.
        return 0;
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
            meat_hotspot: Vec3::ZERO,
            meat_val: 0.0,
            carrying: 0.0,
            prey_spotted: 0.0,
            rally_val: 0.0,
            alarm_val: 0.0,
            seen_by_squad: 0.0,
            squad: SquadFields::neutral(),
        }
    }

    fn behavior(mode: Mode, rank: u8, considerations: Vec<Consideration>) -> Behavior {
        Behavior { mode, rank, considerations, target: TargetKind::None }
    }

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1.0e-5, "expected {b}, got {a}");
    }

    #[test]
    fn mode_all_is_dense_and_in_discriminant_order() {
        // `surprise` indexes mode histograms by `Mode::index()` (the discriminant). If a variant is added
        // to the enum but not to `ALL`, or the order drifts, every distribution silently misattributes
        // counts. Catch it here instead.
        for (i, mode) in Mode::ALL.iter().enumerate() {
            assert_eq!(mode.index(), i, "{mode:?} must sit at index {i} of Mode::ALL");
        }
        assert_eq!(MODE_COUNT, Mode::ALL.len());
    }

    #[test]
    fn curve_linear_clamps_to_unit_range() {
        let c = Curve::Linear { m: 1.0, b: 0.0 };
        approx(c.eval(0.5), 0.5);
        approx(c.eval(2.0), 1.0); // clamped high
        approx(c.eval(-1.0), 0.0); // clamped low
    }

    #[test]
    fn curve_logistic_and_step() {
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
        // index 0. This repertoire is exactly what `validate_unconditional_default` REJECTS at startup,
        // so the branch is unreachable in the shipped game; the test pins `decide`'s total behaviour for
        // the invariant-violated case rather than endorsing it as a runtime fallback.
        let behaviors = vec![behavior(
            Mode::Flee,
            2,
            vec![Consideration {
                input: Input::Perc(Fact::SelfHealthFrac),
                curve: Curve::Linear { m: 1.0, b: 0.0 },
            }],
        )];
        assert!(validate_unconditional_default(&behaviors, "test").is_err());
        let perc = zeroed(); // health_frac = 0.0
        let mut rng = 7u32;
        assert_eq!(decide(&behaviors, &perc, &mut rng), 0);
    }

    #[test]
    fn unconditional_default_accepts_a_constant_floor() {
        // A `Linear { m: 0.0, b }` reads nothing from the world, so its score is a floor the agent always
        // reaches — this is what every shipped brain's Wander/Forage/FollowAnchor tail provides.
        let behaviors = vec![
            behavior(
                Mode::Flee,
                5,
                vec![Consideration {
                    input: Input::Drive(DriveId::FEAR),
                    curve: Curve::Logistic { k: 10.0, x0: 0.5 },
                }],
            ),
            behavior(
                Mode::Wander,
                0,
                vec![Consideration {
                    input: Input::Perc(Fact::SelfHealthFrac),
                    curve: Curve::Linear { m: 0.0, b: 0.12 },
                }],
            ),
        ];
        assert!(validate_unconditional_default(&behaviors, "test").is_ok());
    }

    #[test]
    fn unconditional_default_accepts_a_positively_sloped_linear() {
        // The crab's Forage shape: `0.8 * HUNGER + 0.2`. Every perception input is non-negative, so a
        // non-negative slope means the intercept is a true floor — this behaviour can never be gated off,
        // which is exactly why the crab brain always has a choice.
        let behaviors = vec![behavior(
            Mode::Forage,
            0,
            vec![Consideration {
                input: Input::Drive(DriveId::HUNGER),
                curve: Curve::Linear { m: 0.8, b: 0.2 },
            }],
        )];
        assert!(validate_unconditional_default(&behaviors, "test").is_ok());
    }

    #[test]
    fn unconditional_default_rejects_a_negatively_sloped_linear() {
        // A negative slope over an unbounded input (a distance) decays past MIN_SCORE, so its intercept
        // guarantees nothing.
        let behaviors = vec![behavior(
            Mode::Forage,
            0,
            vec![Consideration {
                input: Input::Perc(Fact::NearestUnitDist),
                curve: Curve::Linear { m: -0.1, b: 0.9 },
            }],
        )];
        assert!(validate_unconditional_default(&behaviors, "test").is_err());
    }

    #[test]
    fn unconditional_default_rejects_a_floor_below_min_score() {
        // A constant floor that does not clear MIN_SCORE cannot claim a rank bucket, so it is not a
        // usable default — `decide` would still find nothing eligible.
        let behaviors = vec![behavior(
            Mode::Wander,
            0,
            vec![Consideration {
                input: Input::Perc(Fact::SelfHealthFrac),
                curve: Curve::Linear { m: 0.0, b: MIN_SCORE * 0.5 },
            }],
        )];
        assert!(validate_unconditional_default(&behaviors, "test").is_err());
    }

    #[test]
    fn unconditional_default_rejects_an_all_gated_repertoire() {
        // Every behaviour hangs off a Step gate the world can switch off → no guaranteed floor.
        let behaviors = vec![
            behavior(
                Mode::Overwatch,
                4,
                vec![Consideration {
                    input: Input::Perc(Fact::ThreatBearingKnown),
                    curve: Curve::Step { threshold: 0.5, below: 0.0, above: 1.0 },
                }],
            ),
            behavior(
                Mode::Examine,
                4,
                vec![Consideration {
                    input: Input::Perc(Fact::HasUnexaminedNearby),
                    curve: Curve::Step { threshold: 0.5, below: 0.0, above: 1.0 },
                }],
            ),
        ];
        assert!(validate_unconditional_default(&behaviors, "test").is_err());
    }

    #[test]
    fn empty_repertoire_is_rejected() {
        assert!(validate_unconditional_default(&[], "test").is_err());
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
