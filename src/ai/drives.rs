//! Drives — the per-agent **needs** a utility brain weighs (hunger, fear). Drives are an **open,
//! data-defined set**, not a hardcoded struct: a drive is an index newtype ([`DriveId`]) into a
//! fixed-width array, and its update behaviour is a [`DriveRule`] in a registry. Built-in rules cover the
//! shapes the current drives need.
//!
//! Adding a drive = a `DriveId` const + bump [`DRIVE_COUNT`] + one `DriveDef` literal in the registry
//! builder (`super::init_drives`) + optional RON knobs. Nothing else changes — considerations read it
//! by id, and every creature carries the full array.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::faction::{Faction, FACTION_COUNT};
use super::field::{FieldId, Stig};
use crate::dungeon::Dungeon;

/// A need, addressed by a stable slot index. Extend by adding a const + bumping [`DRIVE_COUNT`].
/// `Deserialize` so squad role brains can name a drive (`Drive((2))` = CURIOSITY) in `roles.ron`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Serialize)]
pub struct DriveId(pub usize);

impl DriveId {
    pub const HUNGER: DriveId = DriveId(0);
    pub const FEAR: DriveId = DriveId(1);
    // NOTE: a CROWDING drive (tracking CRAB_DENSITY) was removed — it was updated every tick for every
    // crab but read by no behaviour (pure waste), and Reynolds separation in `crab_locomotion` already
    // provides the physical dispersal it was meant to model. Breeding reads the CRAB_DENSITY *field*
    // directly (`nest_reproduce`), not a per-agent drive.

    // --- Squad-unit drives (used by role brains; crabs & the boss carry the slots but ignore them). ---
    /// Rises near unexamined things → pushes the Researcher to Examine.
    pub const CURIOSITY: DriveId = DriveId(2);
    /// Rises with distance from the squad anchor → pulls a strayed unit to Regroup.
    pub const COHESION: DriveId = DriveId(3);
    /// Squad morale; damped by fear, restored by the Psionic's Ward and by regrouping.
    pub const MORALE: DriveId = DriveId(4);
}

/// Number of drive slots. Bump when adding a [`DriveId`]. Slots 2–4 are squad-unit drives; crabs and
/// the boss carry the wider array but their brains reference only HUNGER/FEAR.
pub const DRIVE_COUNT: usize = 5;

/// Per-agent need scalars, each clamped to `[0, 1]`. Every creature carries the full array; a brain
/// reads only the drives its behaviours reference.
#[derive(Component)]
pub struct Drives {
    pub v: [f32; DRIVE_COUNT],
}

impl Drives {
    pub fn new() -> Self {
        Self {
            v: [0.0; DRIVE_COUNT],
        }
    }
    /// Construct with one drive pre-seeded (the rest zero) — used to spread crabs' starting HUNGER so the
    /// swarm isn't a lock-stepped uniform ramp (some spawn hungry and press, some spawn sated and forage).
    pub fn seeded(id: DriveId, value: f32) -> Self {
        let mut d = Self::new();
        d.v[id.0] = value.clamp(0.0, 1.0);
        d
    }
    #[inline]
    pub fn get(&self, id: DriveId) -> f32 {
        self.v[id.0]
    }
    /// Set one drive, clamped to `[0,1]`. Used by gameplay systems that sate/spike a need directly (e.g.
    /// feeding draining HUNGER), separate from the per-tick `DriveRule` updates.
    #[inline]
    pub fn set(&mut self, id: DriveId, value: f32) {
        self.v[id.0] = value.clamp(0.0, 1.0);
    }
}

/// How fast a `TrackMaxFields` drive eases toward the field-driven target (per second) — gives fear a
/// believable rise-lag and a decay when the danger evaporates.
const TRACK_EASE: f32 = 3.0;

/// A pluggable update rule for one drive. Type-safe enum (not `Box<dyn>`).
pub enum DriveRule {
    /// Accumulate over time toward 1 (hunger).
    RiseOverTime { rate: f32 },
    /// Ease toward `min(max_i(sample(field_i) * gain_i), 1)` — the fear rule.
    ///
    /// One entry per *enemy* threat channel: an agent fears what other factions emit, never its own
    /// emissions (see [`super::faction`]). The reduction is `max`, not a sum, for two reasons: a sum would
    /// make two mild dangers add up to panic, and — since it is commutative *and* associative in exact
    /// float arithmetic — `max` keeps the result independent of slice order, which a running float sum
    /// would not (the same determinism concern as `deterministic_centroid`).
    ///
    /// The `(channel, gain)` pairs are **owned** (a one-time Startup allocation), not a `&'static` slice,
    /// because the gains now come from the `sim:` config slice (`SimTuning::fear`) rather than code consts —
    /// so `init_drives` builds them from the loaded config. The `max` reduction is unchanged, so this stays
    /// order-independent and determinism-neutral.
    TrackMaxFields { sources: Vec<(FieldId, f32)> },
}

/// The level a `TrackMaxFields` drive eases toward, given each source's `(sample, gain)`.
///
/// Split out as a pure function so the fear model is unit-testable without a `Stig` grid or an ECS. The
/// reduction is `max` rather than a sum: two mild dangers must not add up to panic, and `max` is
/// order-independent in exact float arithmetic where a running sum is not — the same determinism
/// requirement that forces `deterministic_centroid` to value-sort its addends.
pub fn track_max_target(samples: impl IntoIterator<Item = (f32, f32)>) -> f32 {
    samples
        .into_iter()
        .map(|(sample, gain)| sample * gain)
        .fold(0.0f32, f32::max)
        .min(1.0)
}

impl DriveRule {
    /// The drive's next value under this rule, before clamping.
    fn step(&self, prev: f32, dt: f32, stig: &Stig, dungeon: &Dungeon, pos: Vec3) -> f32 {
        match self {
            DriveRule::RiseOverTime { rate } => prev + rate * dt,
            DriveRule::TrackMaxFields { sources } => {
                let target = track_max_target(
                    sources.iter().map(|&(field, gain)| (stig.sample(field, dungeon, pos), gain)),
                );
                prev + (target - prev) * (TRACK_EASE * dt).min(1.0)
            }
        }
    }
}

/// One drive's identity + update rule. Numeric knobs (rate/gain) come from the `ai_tuning:` config slice later.
pub struct DriveDef {
    pub id: DriveId,
    pub rule: DriveRule,
}

/// The active set of drives and how they update, **keyed by faction**. Built at startup (the extension
/// point). Indexed by [`Faction::index`]; a faction with no rules simply has an empty slice.
#[derive(Resource)]
pub struct DriveRegistry {
    pub by_faction: [Vec<DriveDef>; FACTION_COUNT],
}

impl DriveRegistry {
    pub fn defs(&self, faction: Faction) -> &[DriveDef] {
        &self.by_faction[faction.index()]
    }
}

/// Advance every agent's drives by the rules registered for **its faction**. Cheap (a few float ops +
/// field samples per agent); runs in `AiSet::Drives`, before decisions.
///
/// Faction-keyed so nothing reads its own emissions: a unit tracks the crab/anomaly threat channels, a
/// crab tracks gunfire. `Faction` is required on every `Drives` carrier — `faction::validate_factions`
/// panics at startup otherwise, rather than letting the query silently drop a fearless agent.
pub fn update_drives(
    time: Res<Time>,
    stig: Res<Stig>,
    dungeon: Res<Dungeon>,
    registry: Res<DriveRegistry>,
    mut agents: Query<(&Transform, &Faction, &mut Drives)>,
) {
    let dt = time.delta_secs();
    for (tf, faction, mut drives) in &mut agents {
        for def in registry.defs(*faction) {
            let prev = drives.v[def.id.0];
            let next = def.rule.step(prev, dt, &stig, &dungeon, tf.translation);
            drives.v[def.id.0] = next.clamp(0.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    // Pure drive math — no App, no ECS, no Stig grid (the seed-in/assert-out convention of `wfc.rs`).
    use super::*;

    #[test]
    fn track_max_takes_the_scariest_source_not_the_sum() {
        // Two mild dangers must not add up to panic: a unit beside two separate half-scary things is as
        // afraid as the scarier one, no more. A sum would read 0.9 here and trip the Flee gate.
        assert_eq!(track_max_target([(1.0, 0.5), (1.0, 0.4)]), 0.5);
    }

    #[test]
    fn track_max_is_independent_of_source_order() {
        // The reduction feeds FEAR, which feeds mode selection, which feeds movement — and movement is
        // hashed. `max` is order-independent where a running float sum would not be, so a reordered
        // source slice can never perturb the replay hash.
        let a = track_max_target([(0.3, 0.7), (0.9, 0.11), (0.5, 0.5)]);
        let b = track_max_target([(0.5, 0.5), (0.3, 0.7), (0.9, 0.11)]);
        let c = track_max_target([(0.9, 0.11), (0.5, 0.5), (0.3, 0.7)]);
        assert_eq!(a.to_bits(), b.to_bits());
        assert_eq!(b.to_bits(), c.to_bits());
    }

    #[test]
    fn track_max_is_monotonic_in_danger_and_saturates() {
        assert_eq!(track_max_target([(0.0, 0.9)]), 0.0, "no danger → no fear");
        assert!(track_max_target([(2.0, 0.1)]) > track_max_target([(1.0, 0.1)]));
        assert_eq!(track_max_target([(100.0, 0.9)]), 1.0, "fear saturates at 1");
    }

    #[test]
    fn track_max_of_nothing_is_zero() {
        // A faction with no fear sources (the watcher) eases toward 0, not toward some sentinel.
        assert_eq!(track_max_target([]), 0.0);
    }
}
