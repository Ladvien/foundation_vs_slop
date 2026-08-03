//! **The quarantine cordon, made visible** (FVS-K-1's second clause: *"quarantine has readable
//! feedback"*).
//!
//! # What was missing
//!
//! [`super::area::tick_quarantine`] has been correct since it was written. It was also completely
//! invisible. `selection::place_quarantine_input` spawned `(run_scoped(), Quarantine { radius },
//! Transform)` — no mesh, no decal, no gizmo — and a repo-wide search for `&Quarantine` returned
//! exactly one hit: the system's own query. The player buys a charge for 50 O5 budget, gets **one**
//! per expedition (`config.ron: quarantine_supply: 1`), and drops a 3 m circle they cannot see, at a
//! spot they could not preview, around an anomaly that gives no sign of being held.
//!
//! Worse at the other end: a **breach** was the one containment event with no surface at all. The
//! containment HUD only renders `Phase::BeingContained`, so when a bloom leaves the cordon the panel
//! does not say anything — it *vanishes*, which reads as a UI glitch rather than as the run's most
//! important setback.
//!
//! # Four surfaces, and why each is the cheap one
//!
//! 1. [`preview_cordon`] — the armed footprint under the cursor, snapped through the same
//!    `nearest_floor` call the placement itself uses, so the preview cannot lie about where the
//!    charge lands.
//! 2. [`draw_cordons`] — the placed ring, brightening while it holds something.
//! 3. [`mark_held_anomalies`] — a ring on the anomaly itself, so the player can tell *which* thing
//!    the bottom-left readout is talking about (`update_readout` shows the first `BeingContained`
//!    and names no target).
//! 4. [`report_cordon_events`] — one event-line beat per transition, and the audio.
//!
//! On (4), the budget is evidence-based and it is why this reports **two** things and not five.
//! Ancker et al. 2017 (10.1186/s12911-017-0430-8, already `docs/ui.md` §3.4's source and
//! `ui::event_line`'s founding citation) measured advisory acceptance dropping **~30% per additional
//! alert per encounter**, with no recovery over time — the mechanism is uninformative volume, not
//! desensitisation, so the fix is deleting low-information alerts rather than making them quieter.
//! "Cordon placed" is therefore **not** a beat: the player just clicked, the ring just appeared, and
//! a line telling them what they did is exactly the uninformative kind. Sealed and breached are
//! things the *world* did while they were looking somewhere else.
//!
//! # Determinism
//!
//! Windowed-only, `Update`, and it writes no sim state — [`Gizmos`] is not registered in the headless
//! harness at all, and the phase edges are *read* from `Containment`, never driven. Nothing here can
//! reach `snapshot_hash`.

use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use super::{ArmedTool, Containment, Phase, Quarantine, Quarantinable};
use crate::audio::Sfx;
use crate::palette;
use crate::ui::event_line::GameEvent;

/// Height above the floor the rings are drawn at, so they sit on the surface rather than z-fighting
/// it. Same trick and roughly the same offset `selection::draw_selection_rings` uses.
const RING_LIFT: f32 = 0.04;
/// The held-anomaly mark, in metres. Smaller than any cordon so the two never read as one shape.
const HELD_MARK_RADIUS: f32 = 0.9;

/// Windowed-only. Registered from `ui`, alongside the rest of the player-facing containment surface.
pub struct CordonFeedbackPlugin;

impl Plugin for CordonFeedbackPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (preview_cordon, draw_cordons, mark_held_anomalies, report_cordon_events)
                .distributive_run_if(in_state(crate::session::RunState::Active)),
        );
    }
}

/// Draw a flat ring on the floor. One helper so the three call sites cannot drift on lift or plane.
fn ground_ring(gizmos: &mut Gizmos, at: Vec3, radius: f32, color: Color) {
    let iso = Isometry3d::new(at + Vec3::Y * RING_LIFT, Quat::from_rotation_x(-FRAC_PI_2));
    gizmos.circle(iso, radius, color);
}

/// The armed footprint under the cursor.
///
/// Deliberately drawn at the **snapped** cell centre rather than the raw ground point. Placement
/// snaps to `nearest_floor`, so a preview drawn at the cursor would sit somewhere the cordon will not
/// be — which is worse than no preview, because it is a promise the game then breaks.
fn preview_cordon(
    mut gizmos: Gizmos,
    armed: Res<ArmedTool>,
    tuning: Res<crate::sim::SimTuning>,
    supply: Res<super::QuarantineSupply>,
    dungeon: Option<Res<crate::dungeon::Dungeon>>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>>,
) {
    if *armed != ArmedTool::Quarantine || supply.0 == 0 {
        return;
    }
    let (Some(dungeon), Some(window), Some(camera)) = (dungeon, window, camera) else { return };
    let (camera, cam_tf) = *camera;
    let Some(point) = crate::selection::cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };
    let Some(cell) = crate::selection::nearest_floor(&dungeon, dungeon.world_to_cell(point)) else {
        return;
    };
    ground_ring(
        &mut gizmos,
        dungeon.cell_center(cell),
        tuning.containment.quarantine_radius,
        palette::CORDON_ARMED,
    );
}

/// The placed cordon, brightening while it is actually holding something.
///
/// The `holding` test repeats `tick_quarantine`'s own geometry (distance ≤ radius, against
/// `Quarantinable` in `BeingContained`) rather than reading a flag off the cordon, because the cordon
/// carries no such flag — containment state lives on the *anomaly*, which is the right place for it
/// and the reason this is a two-query read instead of one.
fn draw_cordons(
    mut gizmos: Gizmos,
    cordons: Query<(&Quarantine, &Transform)>,
    anomalies: Query<(&Containment, &Transform), (With<Quarantinable>, Without<Quarantine>)>,
) {
    for (quarantine, tf) in &cordons {
        let holding = anomalies.iter().any(|(containment, anomaly_tf)| {
            containment.phase() == Phase::BeingContained
                && tf.translation.distance(anomaly_tf.translation) <= quarantine.radius
        });
        let color = if holding { palette::CORDON_HOLDING } else { palette::CORDON_IDLE };
        ground_ring(&mut gizmos, tf.translation, quarantine.radius, color);
    }
}

/// Mark the anomaly currently being held, so the readout has a referent in the world.
fn mark_held_anomalies(mut gizmos: Gizmos, anomalies: Query<(&Containment, &Transform)>) {
    for (containment, tf) in &anomalies {
        if containment.phase() == Phase::BeingContained {
            ground_ring(&mut gizmos, tf.translation, HELD_MARK_RADIUS, palette::CORDON_HOLDING);
        }
    }
}

/// Report the two transitions worth a line, and sound them.
///
/// Edge-detected from a `Local` map rather than from a component, for the same reason the whole
/// module is windowed-only: the edge is a presentation fact. `audio::growl_stinger` and
/// `audio::watcher_stinger` both detect their edges exactly this way.
///
/// The dedupe in `ui::event_line` is per *subject entity* per encounter, so a bloom that is contained,
/// breached and re-contained reports once. That is the alert budget working as designed, not a bug —
/// the second breach is the same information as the first.
fn report_cordon_events(
    anomalies: Query<(Entity, &Containment, &Transform), With<Quarantinable>>,
    mut events: MessageWriter<GameEvent>,
    mut sfx: MessageWriter<Sfx>,
    mut previous: Local<HashMap<Entity, Phase>>,
) {
    let mut seen: HashMap<Entity, Phase> = HashMap::with_capacity(previous.len());
    for (entity, containment, tf) in &anomalies {
        let phase = containment.phase();
        let was = previous.get(&entity).copied().unwrap_or(Phase::Uncontained);
        seen.insert(entity, phase);
        if was == phase {
            continue;
        }
        match (was, phase) {
            (Phase::Uncontained, Phase::BeingContained) => {
                events.write(GameEvent::beat(entity, "CORDON SEALED — HOLD THE LINE, QUIETLY"));
                sfx.write(Sfx::CordonSealed(tf.translation));
            }
            // The breach. `tick_quarantine` treats leaving the cordon as a CANCEL, not a lapse, so
            // the hold is already gone — the copy says what was lost rather than what is happening.
            (Phase::BeingContained, Phase::Uncontained) => {
                events.write(GameEvent::beat(entity, "CORDON BREACHED — HOLD LOST"));
                sfx.write(Sfx::CordonBreached(tf.translation));
            }
            // Completion has its own reward, its own HUD change and its own specimen; a line here
            // would be the third telling of one event, which is the volume Ancker et al. measure.
            _ => {}
        }
    }
    // Rebuilt rather than mutated, so a despawned anomaly (run teardown) cannot leak an entry that a
    // recycled `Entity` id would then match against.
    *previous = seen;
}
