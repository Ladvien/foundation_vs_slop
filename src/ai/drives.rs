//! Drives — the per-agent **needs** a utility brain weighs (hunger, fear, fatigue, and esoteric
//! extra-dimensional urges). Drives are an **open, data-defined set**, not a hardcoded struct: a drive
//! is an index newtype ([`DriveId`]) into a fixed-width array, and its update behaviour is a
//! [`DriveRule`] in a registry. Built-in rules cover the common shapes; [`DriveRule::Custom`] is the
//! escape hatch (a plain `fn`, no alloc, one path) for arbitrary/esoteric drives.
//!
//! Adding a drive = a `DriveId` const + bump [`DRIVE_COUNT`] + one `DriveDef` literal in the registry
//! builder (`super::init_drives`) + optional RON knobs. Nothing else changes — considerations read it
//! by id, and every creature carries the full array (unused slots are simply never read).

use bevy::prelude::*;

use super::field::{FieldId, Stig};
use crate::dungeon::Dungeon;

/// A need, addressed by a stable slot index. Extend by adding a const + bumping [`DRIVE_COUNT`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DriveId(pub usize);

impl DriveId {
    pub const HUNGER: DriveId = DriveId(0);
    pub const FEAR: DriveId = DriveId(1);
    // Wired by later phases (registered in `init_drives` as their behaviours land).
    #[allow(dead_code)]
    pub const FATIGUE: DriveId = DriveId(2);
    #[allow(dead_code)]
    pub const BLOODLUST: DriveId = DriveId(3); // boss: drawn to the biggest blood/scent frenzy
    #[allow(dead_code)]
    pub const LIBIDO: DriveId = DriveId(4); // reproduction: well-fed → spawn
    #[allow(dead_code)]
    pub const CROWDING: DriveId = DriveId(5); // territorial: reads CRAB_DENSITY
}

/// Number of drive slots. Bump when adding a [`DriveId`].
pub const DRIVE_COUNT: usize = 6;

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
    #[inline]
    pub fn get(&self, id: DriveId) -> f32 {
        self.v[id.0]
    }
}

/// How fast a `TrackField` drive eases toward the field-driven target (per second) — gives fear a
/// believable rise-lag and a decay when the danger evaporates.
const TRACK_EASE: f32 = 3.0;

/// Read-only context a [`DriveRule::Custom`] sees. No `&mut world`, so custom rules are pure and
/// deterministic. Extend with more perception as esoteric drives need it. (Consumed once a `Custom`
/// drive is registered — the escape hatch is part of the public extension surface.)
#[allow(dead_code)]
pub struct DriveCtx<'a> {
    pub prev: f32,
    pub dt: f32,
    pub stig: &'a Stig,
    pub dungeon: &'a Dungeon,
    pub pos: Vec3,
}

/// A pluggable update rule for one drive. `fn` pointers (not `Box<dyn>`) keep it type-safe + alloc-free.
pub enum DriveRule {
    /// Accumulate over time toward 1 (hunger, fatigue).
    RiseOverTime { rate: f32 },
    /// Ease toward `min(field_sample * gain, 1)` — fear←THREAT, bloodlust←SCENT, crowding←CRAB_DENSITY.
    TrackField { field: FieldId, gain: f32 },
    /// Arbitrary rule for esoteric extra-dimensional drives; returns the new clamped value. The
    /// escape hatch — part of the extension surface, exercised as custom drives are added.
    #[allow(dead_code)]
    Custom(fn(&DriveCtx) -> f32),
}

/// One drive's identity + update rule. Numeric knobs (rate/gain) come from `ai_tuning.ron` later.
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
                DriveRule::Custom(f) => f(&DriveCtx {
                    prev,
                    dt,
                    stig: &stig,
                    dungeon: &dungeon,
                    pos: tf.translation,
                }),
            };
            drives.v[def.id.0] = next.clamp(0.0, 1.0);
        }
    }
}
