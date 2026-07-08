//! Drives — the per-agent **needs** a utility brain weighs (hunger, fear). Drives are an **open,
//! data-defined set**, not a hardcoded struct: a drive is an index newtype ([`DriveId`]) into a
//! fixed-width array, and its update behaviour is a [`DriveRule`] in a registry. Built-in rules cover the
//! shapes the current drives need.
//!
//! Adding a drive = a `DriveId` const + bump [`DRIVE_COUNT`] + one `DriveDef` literal in the registry
//! builder (`super::init_drives`) + optional RON knobs. Nothing else changes — considerations read it
//! by id, and every creature carries the full array.

use bevy::prelude::*;
use serde::Deserialize;

use super::field::{FieldId, Stig};
use crate::dungeon::Dungeon;

/// A need, addressed by a stable slot index. Extend by adding a const + bumping [`DRIVE_COUNT`].
/// `Deserialize` so squad role brains can name a drive (`Drive((2))` = CURIOSITY) in `roles.ron`.
#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
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

/// How fast a `TrackField` drive eases toward the field-driven target (per second) — gives fear a
/// believable rise-lag and a decay when the danger evaporates.
const TRACK_EASE: f32 = 3.0;

/// A pluggable update rule for one drive. `fn` pointers (not `Box<dyn>`) keep it type-safe + alloc-free.
pub enum DriveRule {
    /// Accumulate over time toward 1 (hunger).
    RiseOverTime { rate: f32 },
    /// Ease toward `min(field_sample * gain, 1)` — fear←THREAT.
    TrackField { field: FieldId, gain: f32 },
}

/// One drive's identity + update rule. Numeric knobs (rate/gain) come from the `ai_tuning:` config slice later.
pub struct DriveDef {
    pub id: DriveId,
    pub rule: DriveRule,
}

/// The active set of drives and how they update. Built at startup (the extension point).
#[derive(Resource)]
pub struct DriveRegistry {
    pub defs: Vec<DriveDef>,
}

/// Advance every agent's drives by each registered rule. Cheap (a few float ops + field samples per
/// agent); runs in `AiSet::Drives`, before decisions.
pub fn update_drives(
    time: Res<Time>,
    stig: Res<Stig>,
    dungeon: Res<Dungeon>,
    registry: Res<DriveRegistry>,
    mut agents: Query<(&Transform, &mut Drives)>,
) {
    let dt = time.delta_secs();
    for (tf, mut drives) in &mut agents {
        for def in &registry.defs {
            let prev = drives.v[def.id.0];
            let next = match def.rule {
                DriveRule::RiseOverTime { rate } => prev + rate * dt,
                DriveRule::TrackField { field, gain } => {
                    let target = (stig.sample(field, &dungeon, tf.translation) * gain).min(1.0);
                    prev + (target - prev) * (TRACK_EASE * dt).min(1.0)
                }
            };
            drives.v[def.id.0] = next.clamp(0.0, 1.0);
        }
    }
}
