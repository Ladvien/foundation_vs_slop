//! Dev instrument: a **fruit-body testbed**, for the one question a screenshot cannot answer — is any part
//! of a mushroom inside a wall?
//!
//! Wall clipping is easy to see and hard to see *reliably*. A mushroom's cap can overlap a slab by a
//! centimetre from one camera angle and look perfectly seated from another, and the bodies that clip are the
//! ones planted against skirting in the dark corners of a 192×192 dungeon, which is exactly where nobody is
//! looking. Eyeballing it is how the bug survived two rounds of "fixed".
//!
//! So this plants a row of **adult** bodies at measured distances from a real wall face, runs the same
//! [`super::fruit::plan_body`] the live spawner uses, and then asks [`super::fruit::penetration`] how deep
//! the resulting pose actually sits inside solid matter. The answer is a number, printed once and asserted
//! against zero. The screenshot is then a courtesy, not the evidence.
//!
//! ```sh
//! MYCELIA_FRUIT_TESTBED=1 cargo run
//! ```
//!
//! Off by default and gated on the environment variable, because it spawns bodies the mold never grew and
//! parks the camera on them.

use std::env;

use bevy::prelude::*;

use crate::dungeon::Dungeon;

use super::fruit::{penetration, plan_body, BodyPlan, DeathCapScene, FruitBody};
use super::perceptual::CAP_RADIUS_M;

/// Environment variable that arms the testbed.
const ENV_FLAG: &str = "MYCELIA_FRUIT_TESTBED";

/// Distances (world units) from the wall face at which to plant a body's *pin site* — i.e. before
/// `plan_body` gets to move it. The first few are inside the slab's own reach: a cap of radius
/// `CAP_RADIUS_M * scale` (22 cm at the shipped scale) overhangs all of them.
const SITE_OFFSETS: [f32; 6] = [0.02, 0.06, 0.12, 0.18, 0.26, 0.40];

/// Scale to plant them at. Fixed rather than jittered, so the row is a controlled comparison.
const TESTBED_SCALE: f32 = 4.0;

pub(super) fn build(app: &mut App) {
    if env::var(ENV_FLAG).is_err() {
        return;
    }
    warn!("mycelia: {ENV_FLAG} is set — planting a fruit-body testbed row. Not a shipping configuration.");
    app.add_systems(Startup, plant.after(super::setup_mycelia));
    app.add_systems(Update, audit);
}

/// Re-check every mushroom the mold actually grew.
///
/// The planted row proves `plan_body` solves the cases the row poses. This proves the *live spawner* — with
/// its jittered scales, its yaws, and whatever wall it happened to pin against — produces poses that are
/// genuinely clear. It is the difference between testing the function and testing the game.
///
/// `FruitBody` stores `bend`/`tilt` in object space, so the entity's yaw has to be put back before the pose
/// can be compared against world geometry.
fn audit(
    time: Res<bevy::time::Time<bevy::time::Real>>,
    mut next_at: Local<f32>,
    dungeon: Res<Dungeon>,
    bodies: Query<(&FruitBody, &Transform)>,
) {
    let now = time.elapsed_secs();
    if now < *next_at {
        return;
    }
    *next_at = now + 10.0;
    if bodies.is_empty() {
        return;
    }

    let mut worst = 0.0f32;
    let mut offender = Vec2::ZERO;
    let mut tilts: Vec<f32> = Vec::new();
    let mut scales: Vec<f32> = Vec::new();
    for (body, transform) in &bodies {
        let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
        let reyaw = |v: Vec2| {
            let r = Quat::from_rotation_y(yaw) * Vec3::new(v.x, 0.0, v.y);
            Vec2::new(r.x, r.z)
        };
        let plan = BodyPlan {
            base: Vec2::new(transform.translation.x, transform.translation.z),
            bend: reyaw(body.bend),
            tilt: reyaw(body.tilt),
        };
        let depth = penetration(&dungeon, &plan, body.scale);
        if depth > worst {
            worst = depth;
            offender = plan.base;
        }
        tilts.push(body.tilt.length().atan().to_degrees());
        scales.push(body.scale);
    }

    let n = bodies.iter().count();
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    let span = |v: &[f32]| {
        let lo = v.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        (lo, hi)
    };
    let (tlo, thi) = span(&tilts);
    let (slo, shi) = span(&scales);
    if worst > 0.0 {
        error!(
            "mycelia audit: {n} live bodies, WORST PENETRATION {worst:.4} m at {offender:?} — the live \
             spawner is planting mushrooms inside walls."
        );
    } else {
        info!(
            "mycelia audit: {n} live bodies, all clear. tilt {tlo:.1}–{thi:.1}° (mean {:.1}), \
             scale {slo:.2}–{shi:.2} (mean {:.2})",
            mean(&tilts),
            mean(&scales),
        );
    }
}

/// Spacing between bodies along the wall, world units. Wide enough that their caps never meet.
const ROW_SPACING: f32 = 0.7;

/// Find a **usable** stretch of wall near the squad spawn.
///
/// "Usable" is doing real work here. The first version of this only found a wall face, and planted the row
/// at ±7.5 m along it — straight out of the room and into rock, where `march_out` never escapes and every
/// body reports the same `MARCH_MAX` penetration. The one site that happened to land on floor was 1.5 m from
/// any wall, took `plan_body`'s no-wall early-out, and reported a clean 0. So the run "found five clipping
/// bodies and one good one" while testing nothing at all.
///
/// A stretch qualifies only if, for every body the row will plant, the *pin site* lies on open floor **and**
/// the slab is genuinely present behind it. Otherwise the testbed is grading its own homework.
fn find_wall_run(dungeon: &Dungeon) -> Option<(Vec2, Vec2)> {
    let spawn = dungeon.world_to_cell(dungeon.spawn_world());
    let half = (SITE_OFFSETS.len() as f32 - 1.0) * 0.5;

    // Spiral outward so the row lands somewhere the startup camera is already looking.
    for ring in 1..24i32 {
        for dx in -ring..=ring {
            for dz in -ring..=ring {
                if dx.abs() != ring && dz.abs() != ring {
                    continue;
                }
                let cell = spawn + IVec2::new(dx, dz);
                if !dungeon.is_floor(cell) {
                    continue;
                }
                for (step, inward) in [
                    (IVec2::X, Vec2::new(-1.0, 0.0)),
                    (IVec2::NEG_X, Vec2::new(1.0, 0.0)),
                    (IVec2::Y, Vec2::new(0.0, -1.0)),
                    (IVec2::NEG_Y, Vec2::new(0.0, 1.0)),
                ] {
                    if dungeon.is_floor(cell + step) {
                        continue;
                    }
                    // The slab's inner face sits `WALL_THICKNESS` inside the cell boundary.
                    let boundary = Vec2::new(cell.x as f32, cell.y as f32)
                        + Vec2::new(step.x as f32, step.y as f32) * 0.5;
                    let face = boundary + inward * crate::dungeon::WALL_THICKNESS;
                    let along = Vec2::new(-inward.y, inward.x);

                    let usable = SITE_OFFSETS.iter().enumerate().all(|(i, offset)| {
                        let lateral = along * ((i as f32 - half) * ROW_SPACING);
                        let site = face + inward * *offset + lateral;
                        // The pin site itself must be on open floor...
                        !super::control::solid_at_world(dungeon, site)
                            // ...and the slab must actually be there to lean away from.
                            && super::control::solid_at_world(dungeon, face - inward * 0.02 + lateral)
                    });
                    if usable {
                        return Some((face, inward));
                    }
                }
            }
        }
    }
    None
}

/// Diagonal distances (world units) from an inside corner at which to plant a body's pin site.
const CORNER_OFFSETS: [f32; 3] = [0.04, 0.14, 0.28];

/// Find an inside corner near the spawn: a floor cell with two *adjacent* non-floor neighbours.
///
/// Corners are the case a single escape direction handles worst. Pushing a body out along the diagonal
/// clears each of the two faces by only `1/√2` of the distance travelled, so a solver that reasons about
/// "the nearest wall" and pushes once will under-clear both. The row along a flat wall would never catch it.
///
/// Returns the corner's inner point (where the two slab faces meet) and the outward diagonal.
fn find_corner(dungeon: &Dungeon) -> Option<(Vec2, Vec2)> {
    let spawn = dungeon.world_to_cell(dungeon.spawn_world());
    let t = crate::dungeon::WALL_THICKNESS;
    for ring in 1..24i32 {
        for dx in -ring..=ring {
            for dz in -ring..=ring {
                if dx.abs() != ring && dz.abs() != ring {
                    continue;
                }
                let cell = spawn + IVec2::new(dx, dz);
                if !dungeon.is_floor(cell) {
                    continue;
                }
                for (sx, sz) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
                    let (nx, nz) = (IVec2::new(sx, 0), IVec2::new(0, sz));
                    if dungeon.is_floor(cell + nx) || dungeon.is_floor(cell + nz) {
                        continue;
                    }
                    // Both faces present: the inner corner is inset by the slab thickness on both axes.
                    let corner = Vec2::new(
                        cell.x as f32 + sx as f32 * (0.5 - t),
                        cell.y as f32 + sz as f32 * (0.5 - t),
                    );
                    let out = Vec2::new(-(sx as f32), -(sz as f32)).normalize();
                    // Only useful if the deepest site we plant is still on open floor.
                    let deepest = corner + out * CORNER_OFFSETS[0];
                    if !super::control::solid_at_world(dungeon, deepest) {
                        return Some((corner, out));
                    }
                }
            }
        }
    }
    None
}

/// Plant the row, and report what the geometry actually does.
fn plant(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    scene: Res<DeathCapScene>,
) -> Result<(), BevyError> {
    let Some((face, inward)) = find_wall_run(&dungeon) else {
        return Err("mycelia testbed: found no usable stretch of wall near the squad spawn".into());
    };
    // Run the row *along* the wall, so each body gets its own patch of slab.
    let along = Vec2::new(-inward.y, inward.x);
    let cap_r = CAP_RADIUS_M * TESTBED_SCALE;

    info!(
        "mycelia testbed: wall face at {face:?}, inward {inward:?}. Adult cap radius is {:.3} m, so every \
         site closer than that overhangs the slab and must be carried clear by the stem.",
        cap_r
    );

    let mut worst = 0.0f32;
    let half = (SITE_OFFSETS.len() as f32 - 1.0) * 0.5;
    for (i, offset) in SITE_OFFSETS.iter().enumerate() {
        let site = face + inward * *offset + along * ((i as f32 - half) * ROW_SPACING);
        let seed = 0xF00D + i as u32;
        let Some(plan) = plan_body(&dungeon, site, TESTBED_SCALE, seed) else {
            return Err(format!("mycelia testbed: no clear pose for site {i} at {site:?}").into());
        };
        let depth = penetration(&dungeon, &plan, TESTBED_SCALE);
        worst = worst.max(depth);

        let moved = (plan.base - site).length();
        info!(
            "  site {i}: {offset:.2} m from face -> base moved {moved:.3} m, bend {:.3} m, tilt {:.3} \
             ({:.1}°), penetration {depth:.4} m {}",
            plan.bend.length() * TESTBED_SCALE,
            plan.tilt.length(),
            plan.tilt.length().atan().to_degrees(),
            if depth > 0.0 { "<-- CLIPS" } else { "" },
        );

        let yaw = 0.0;
        commands.spawn((
            Name::new(format!("mycelia_testbed_{i}")),
            FruitBody {
                growth: 1.0,
                rise: 1.0,
                scale: TESTBED_SCALE,
                cell: dungeon.world_to_cell(Vec3::new(plan.base.x, 0.0, plan.base.y)),
                veil_triggered: true,
                tint: 1.0,
                bend: plan.bend,
                tilt: plan.tilt,
            },
            Transform::from_translation(Vec3::new(plan.base.x, 0.0, plan.base.y))
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::splat(TESTBED_SCALE)),
            Visibility::default(),
            WorldAssetRoot(scene.handle()),
        ));
    }

    // ── Corners ───────────────────────────────────────────────────────────────────────────────────────
    // The case a single escape direction serves worst.
    match find_corner(&dungeon) {
        None => warn!("mycelia testbed: no inside corner near spawn; corner cases went untested"),
        Some((corner, out)) => {
            info!("mycelia testbed: inside corner at {corner:?}, outward diagonal {out:?}");
            for (i, offset) in CORNER_OFFSETS.iter().enumerate() {
                let site = corner + out * *offset;
                let seed = 0xC0FFEE + i as u32;
                let Some(plan) = plan_body(&dungeon, site, TESTBED_SCALE, seed) else {
                    error!("mycelia testbed: no clear pose for corner site {i} at {site:?}");
                    continue;
                };
                let depth = penetration(&dungeon, &plan, TESTBED_SCALE);
                worst = worst.max(depth);
                info!(
                    "  corner {i}: {offset:.2} m out -> base moved {:.3} m, bend {:.3} m, penetration \
                     {depth:.4} m {}",
                    (plan.base - site).length(),
                    plan.bend.length() * TESTBED_SCALE,
                    if depth > 0.0 { "<-- CLIPS" } else { "" },
                );
                commands.spawn((
                    Name::new(format!("mycelia_testbed_corner_{i}")),
                    FruitBody {
                        growth: 1.0,
                        rise: 1.0,
                        scale: TESTBED_SCALE,
                        cell: dungeon.world_to_cell(Vec3::new(plan.base.x, 0.0, plan.base.y)),
                        veil_triggered: true,
                        tint: 1.0,
                        bend: plan.bend,
                        tilt: plan.tilt,
                    },
                    Transform::from_translation(Vec3::new(plan.base.x, 0.0, plan.base.y))
                        .with_scale(Vec3::splat(TESTBED_SCALE)),
                    Visibility::default(),
                    WorldAssetRoot(scene.handle()),
                ));
            }
        }
    }

    // The whole point of the testbed. A body may lean, curve, and sit with its volva against the skirting —
    // but no part of it may be inside the wall.
    if worst > 0.0 {
        return Err(format!(
            "mycelia testbed: a fruit body penetrates a wall by {worst:.4} m. `plan_body` is not clearing \
             the silhouette it planned for."
        )
        .into());
    }
    info!(
        "mycelia testbed: all {} bodies clear the geometry.",
        SITE_OFFSETS.len() + CORNER_OFFSETS.len()
    );
    Ok(())
}
