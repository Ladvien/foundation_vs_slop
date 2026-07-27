//! **The extraction point** — the second half of [`crate::session::WinCondition::ExtractContained`].
//!
//! Containing an anomaly is not securing it. The run is won when the squad walks a capture *out*, and
//! this module is the place they walk to.
//!
//! **It is the insertion cell**, `Dungeon::spawn` — the same cell `squad::spawn_squad` clusters the
//! five operatives around. You leave the way you came in. That is deliberate on three counts: it needs
//! no new worldgen and no new placement rule; it is legible without a marker ("go back to where you
//! started"); and it is exactly where FVS-G-5's ASYNC door will stand, so the door is this zone with a
//! body rather than a replacement for it.
//!
//! **Determinism.** The zone entity carries a `Transform` but deliberately **no `Health`**, so it
//! contributes no row to `sim_harness::snapshot_hash` (which folds `(Transform, Health)` pairs) and no
//! actor to `liveness_violations`. It is spawned on `OnEnter(RunState::Active)`, not `FixedUpdate`, so
//! it adds no node to the pinned schedule's linearisation.

use bevy::prelude::*;

/// A region the squad must occupy for [`crate::session::WinCondition::ExtractContained`] to resolve.
///
/// Planar (XZ) like every other reach check in this module tree — `device::deploy_devices` and
/// `area::tick_quarantine` both ignore Y, because a unit standing on a floor is at the zone's height by
/// construction and a Y term would only add a way for the check to fail confusingly.
#[derive(Component, Debug, Clone, Copy)]
pub struct ExtractionZone {
    /// Planar radius in world units (metres; 1 tile = 1 m).
    pub radius: f32,
}

impl ExtractionZone {
    /// Is `point` inside a zone centred at `centre`?
    ///
    /// **Inclusive at the boundary**, matching `rule::FieldCondition::is_met`'s convention. A unit
    /// standing exactly on the rim must not see the HUD say "not there" while the win ticks.
    pub fn contains(&self, centre: Vec3, point: Vec3) -> bool {
        (point.xz() - centre.xz()).length() <= self.radius
    }
}

/// Place the run's extraction zone on the dungeon's spawn cell.
///
/// Registered in `RunBuild::Populate` — it reads [`crate::dungeon::Dungeon`], which only exists after
/// `RunBuild::World`.
pub fn spawn_extraction_zone(
    mut commands: Commands,
    dungeon: Res<crate::dungeon::Dungeon>,
    tuning: Res<crate::sim::SimTuning>,
) {
    commands.spawn((
        crate::session::run_scoped(),
        ExtractionZone { radius: tuning.containment.extraction_radius },
        Transform::from_translation(dungeon.cell_center(dungeon.spawn)),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    const Z: ExtractionZone = ExtractionZone { radius: 3.0 };
    const C: Vec3 = Vec3::new(10.0, 0.0, 10.0);

    #[test]
    fn the_zone_predicate_is_inclusive_at_the_boundary() {
        // Exactly on the rim counts. A unit that has walked to the edge has extracted; anything else
        // makes the HUD and the win rule disagree at the one position the player is most likely to
        // stop at.
        assert!(Z.contains(C, C + Vec3::new(3.0, 0.0, 0.0)));
        assert!(Z.contains(C, C));
        assert!(!Z.contains(C, C + Vec3::new(3.001, 0.0, 0.0)));
    }

    #[test]
    fn the_zone_is_planar_so_height_never_decides_an_extraction() {
        // Units sit on the floor and the zone is placed at cell centre height; a Y term could only
        // ever produce a confusing failure, never a useful one.
        assert!(Z.contains(C, C + Vec3::new(0.0, 50.0, 0.0)));
    }
}
