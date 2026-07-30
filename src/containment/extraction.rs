//! **The extraction point** — the second half of [`crate::session::WinCondition::ExtractContained`].
//!
//! Containing an anomaly is not securing it. The run is won when the squad walks a capture *out*, and
//! this module is the place they walk to.
//!
//! **It is the insertion cell**, `Dungeon::spawn` — the same cell `squad::spawn_squad` clusters the
//! five operatives around. You leave the way you came in. That is deliberate on two counts: it needs
//! no new worldgen and no new placement rule, and it is exactly where FVS-G-5's ASYNC door will
//! stand, so the door is this zone with a body rather than a replacement for it.
//!
//! # It needed a marker after all
//!
//! This header used to claim a third count: that the zone is *"legible without a marker ('go back to
//! where you started')"*. It was not. The zone spawned with a `Transform` and nothing else — no
//! mesh, no light, no HUD marker, nothing rendered it anywhere in `src/` — while
//! `ui::verb_bar::objective_line` told the player, in those words, `RETURN TO THE EXTRACTION POINT`.
//! After twenty minutes in a procedurally generated Backrooms level under three-state fog, "where
//! you started" is not a landmark; it is a memory test.
//!
//! [`ExtractionBeaconPlugin`] gives it a body: a standing column of light, visible over the fog's
//! remembered geometry. Lighting rather than a HUD arrow because that is the better-evidenced
//! instrument — Marples 2017 (*The influence of intrinsic perceptual cues on navigation and route
//! selection in virtual environments*, PhD, Huddersfield) derives the thresholds at which guidance
//! lighting begins to bias a player's route, and finds a usable window *below* the level at which
//! they become consciously aware of being guided. A horror game wants exactly that window: the
//! player should find their way out without feeling escorted.
//!
//! It deliberately does **not** reveal anything else. The fog still withholds what is between here
//! and there, which is the spatial half of the ambiguity McCall et al. 2022 (*The Underwood
//! Project*, Behav Res Methods, DOI 10.3758/s13428-022-02002-3) measure dread from.
//!
//! **Determinism.** The zone entity carries a `Transform` but deliberately **no `Health`**, so it
//! contributes no row to `sim_harness::snapshot_hash` (which folds `(Transform, Health)` pairs) and no
//! actor to `liveness_violations`. It is spawned on `OnEnter(RunState::Active)`, not `FixedUpdate`, so
//! it adds no node to the pinned schedule's linearisation.

use bevy::light::NotShadowCaster;
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

/// Marks the beacon's visual entities so they tear down with the run.
#[derive(Component)]
pub struct ExtractionBeacon;

/// How tall the column of light stands, in world units. Tall enough to clear the knee-wall cutaway
/// and read across a room at the default zoom; short enough not to reach the ceiling lights.
const BEACON_HEIGHT: f32 = 6.0;

/// **Windowed-only**: gives the extraction zone something to look at.
///
/// Registered in `lib::run` and never in `sim_harness`, exactly like `psi_vision` and `blood_lens`,
/// so the deterministic core cannot see it. That is not merely tidiness — it is what guarantees the
/// beacon adds no row to `snapshot_hash`: the entities do not exist headless at all. (They also
/// carry no `Health`, so even if they did they would fold to nothing, but "never spawned" is the
/// stronger property and the one worth relying on.)
pub struct ExtractionBeaconPlugin;

impl Plugin for ExtractionBeaconPlugin {
    fn build(&self, app: &mut App) {
        // After `RunBuild::Populate`, because it reads the zone that pass spawns.
        app.add_systems(
            OnEnter(crate::session::RunState::Active),
            spawn_beacon.after(crate::session::RunBuild::Populate),
        );
    }
}

fn spawn_beacon(
    mut commands: Commands,
    zones: Query<(&ExtractionZone, &Transform)>,
    theme: Option<Res<crate::ui::theme::UiTheme>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok((zone, tf)) = zones.single() else {
        // No zone means a win condition that does not use one (`SurviveTicks`). Not an error.
        return;
    };
    // The beacon is the one saturated thing in the palette that is *not* an anomaly, so it takes the
    // UI accent (warm bone) rather than `theme.anomaly` — the extraction point is Foundation
    // infrastructure, and `docs/lore/…scp-color-language.md` §7 reserves colour for deviation.
    let ink = theme.map(|t| t.accent).unwrap_or(Color::srgb(0.95, 0.93, 0.88));
    let material = materials.add(StandardMaterial {
        base_color: ink.with_alpha(0.10),
        emissive: ink.to_linear() * 3.0,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        // The column is a light SHAFT — it must not occlude the squad walking through it, and it
        // must be visible from any of the four iso detents (`camera::ROTATION_STEPS`).
        cull_mode: None,
        ..default()
    });
    let column = meshes.add(Cylinder::new(zone.radius * 0.5, BEACON_HEIGHT));
    let centre = tf.translation;

    commands.spawn((
        crate::session::run_scoped(),
        ExtractionBeacon,
        Mesh3d(column),
        MeshMaterial3d(material),
        NotShadowCaster, // unlit translucent column: casts no shadow (see world::setup_lighting)
        Transform::from_translation(centre + Vec3::Y * (BEACON_HEIGHT * 0.5)),
    ));
    // A real light too, so the pad and the walls near it are lit — that is the part Marples measures
    // as steering a route, rather than the shaft, which is what makes it findable from a distance.
    commands.spawn((
        crate::session::run_scoped(),
        ExtractionBeacon,
        PointLight {
            color: ink,
            intensity: 40_000.0,
            range: 12.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(centre + Vec3::Y * 2.5),
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
