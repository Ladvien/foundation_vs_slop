//! **Rig tripwire** — catches a skinned character whose skeleton has come apart, and *names it*.
//!
//! # Why this exists
//!
//! On 2026-07-29 a player captured two frames of massively stretched geometry — needle-thin spikes
//! and giant black wedges fanning tens of metres across the floor — with the note *"Wtf? No ideae
//! what this is."* (`debug_screenshots/region_2026-07-29_13-12-23-426`). It persisted across two
//! captures 49 seconds apart, so it was not a transient pop.
//!
//! Diagnosing it cost a lot and did not conclude. The leading hypothesis — a hair particle chain
//! diverging when its root teleports — was **disproven** by
//! `hair::tests::a_teleported_root_re_settles_instead_of_exploding`, and a live run reproduced
//! nothing. The surviving evidence is that the stretched triangles are the same cream as the squad's
//! backpacks and shoulder pads, which points at the **character mesh's skinning**.
//!
//! # Why it watches joints rather than the mesh
//!
//! Skinned vertices are computed on the GPU from the joint palette; the CPU never sees them, and a
//! skinned mesh's `Aabb` is its *rest pose* and does not move when skinning explodes. So there is
//! nothing to measure on the mesh side.
//!
//! The joints, however, are ordinary entities with a `GlobalTransform` — and a joint flung far from
//! its owner, or gone non-finite, or scaled to a degenerate value, is *precisely* what stretches the
//! vertices bound to it into the spikes in that capture. Watching the joints is watching the cause.
//!
//! This deliberately does not try to *fix* anything. It converts "wtf is this" into a named
//! diagnosis — which operative, which joint, how far off, at what tick — so the next occurrence
//! arrives with evidence attached instead of costing another investigation.
//!
//! # Cost and safety
//!
//! Debug-only (stripped from release like `devshot`/`region_capture`/`perf_hud`), registered only in
//! `lib::run`, `Update`-only, and it **writes nothing** — it reads transforms and logs. It samples
//! on an interval rather than every frame, and it warns **once per rig** so a persistent break does
//! not flood the log. Nothing here can reach `snapshot_hash`.

use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;

/// How far a joint may sit from its rig's root before it is considered flung.
///
/// The valkyrie figurine is ~1.82 m tall (the manifest's render scale applied to the base mesh), so
/// every legitimate joint is within roughly a metre of the root. 8 m is far outside any pose the rig
/// can reach while still being far below the tens of metres the captured spikes spanned — wide
/// enough that a novel animation cannot trip it, tight enough to fire before the artifact is
/// subtle.
const MAX_JOINT_RADIUS: f32 = 8.0;

/// Seconds between sweeps. This is a tripwire, not a profiler; a break that lasted 49 s in the wild
/// will be caught many times over at this rate, and the cost is ~one transform read per joint per
/// half second.
const SAMPLE_PERIOD: f32 = 0.5;

/// Marks a rig already reported, so a persistent break logs once rather than twice a second forever.
#[derive(Component)]
struct RigBreakReported;

pub struct RigWatchPlugin;

impl Plugin for RigWatchPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, watch_rigs);
    }
}

/// What a sweep found wrong with one joint, if anything.
///
/// Split out as a pure function over the two transforms so the *decision* is unit-testable without
/// an `App`, a GPU, or a rig — the same shape the rest of this codebase uses for anything whose
/// wording or threshold is the deliverable.
fn joint_fault(root: Vec3, joint: &GlobalTransform) -> Option<String> {
    let t = joint.translation();
    if !t.is_finite() {
        return Some(format!("non-finite translation {t:?}"));
    }
    let scale = joint.scale();
    if !scale.is_finite() {
        return Some(format!("non-finite scale {scale:?}"));
    }
    // A zero or near-zero scale collapses every vertex bound to the joint onto a point; a wildly
    // large one is the other half of the same failure. Both read on screen as stretched geometry.
    let max_axis = scale.x.abs().max(scale.y.abs()).max(scale.z.abs());
    if max_axis > 100.0 {
        return Some(format!("degenerate scale {scale:?}"));
    }
    let dist = t.distance(root);
    if !dist.is_finite() || dist > MAX_JOINT_RADIUS {
        return Some(format!("{dist:.1} m from the rig root (limit {MAX_JOINT_RADIUS} m)"));
    }
    None
}

/// Sweep every skinned rig's joints; warn once per rig on the first fault found.
fn watch_rigs(
    time: Res<Time>,
    mut commands: Commands,
    mut next_sweep: Local<f32>,
    // Proves the tripwire is ARMED. Silence otherwise has two readings — "every rig is healthy" and
    // "the query matched nothing, so nothing was ever checked" — and only one of them is good news.
    // Logged once, when rigs first appear.
    mut announced: Local<bool>,
    rigs: Query<(Entity, &SkinnedMesh, &GlobalTransform, Option<&Name>), Without<RigBreakReported>>,
    transforms: Query<&GlobalTransform>,
) {
    let now = time.elapsed_secs();
    if now < *next_sweep {
        return;
    }
    *next_sweep = now + SAMPLE_PERIOD;

    if !*announced {
        let rig_count = rigs.iter().count();
        if rig_count > 0 {
            let joints: usize = rigs.iter().map(|(_, s, _, _)| s.joints.len()).sum();
            info!("rig-watch: armed — watching {rig_count} skinned rig(s), {joints} joints total");
            *announced = true;
        }
    }

    for (rig, skin, rig_tf, name) in &rigs {
        let root = rig_tf.translation();
        if !root.is_finite() {
            warn!("rig-watch: {rig} has a non-finite root transform {root:?}");
            commands.entity(rig).insert(RigBreakReported);
            continue;
        }

        // First fault wins: one clear line beats a wall of collinear ones, and the joint index is
        // enough to find the bone in `docs/artist_guide.md` §4's table.
        //
        // SORT-OK: `joints` is an ordered `Vec` from the glTF loader, iterated in index order —
        // not an ECS query — and this system only logs.
        let broken = skin.joints.iter().enumerate().find_map(|(i, j)| {
            let jt = transforms.get(*j).ok()?;
            joint_fault(root, jt).map(|why| (i, *j, why))
        });

        if let Some((index, joint, why)) = broken {
            let who = name.map(|n| format!(" \"{n}\"")).unwrap_or_default();
            warn!(
                "rig-watch: SKELETON BREAK on {rig}{who} — joint {index} ({joint}) {why}. \
                 {} joints total. This is the shape that renders as stretched spikes across the \
                 level (see debug_screenshots/region_2026-07-29_13-12-23-426 and src/rig_watch.rs). \
                 Root at {root:?}, t={now:.1}s.",
                skin.joints.len()
            );
            commands.entity(rig).insert(RigBreakReported);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32, z: f32) -> GlobalTransform {
        GlobalTransform::from(Transform::from_xyz(x, y, z))
    }

    #[test]
    fn a_joint_in_a_normal_pose_is_not_a_fault() {
        // The whole rig is ~1.8 m tall, so every real pose sits well inside the radius. A tripwire
        // that fires on ordinary animation is worse than none — it would train the reader to ignore
        // the log line that matters.
        let root = Vec3::new(80.0, 0.0, 45.0);
        for offset in [
            Vec3::ZERO,
            Vec3::new(0.0, 1.8, 0.0),   // head
            Vec3::new(0.6, 1.4, 0.3),   // outstretched arm
            Vec3::new(-0.4, 0.1, -0.5), // trailing foot
        ] {
            let j = at(root.x + offset.x, root.y + offset.y, root.z + offset.z);
            assert!(joint_fault(root, &j).is_none(), "offset {offset:?} should be fine");
        }
    }

    #[test]
    fn a_flung_joint_is_caught_and_says_how_far() {
        // The captured spikes spanned tens of metres. This is the case the tripwire exists for.
        let root = Vec3::new(80.0, 0.0, 45.0);
        let why = joint_fault(root, &at(0.0, 0.0, 0.0)).expect("a joint at the world origin is a break");
        assert!(why.contains("from the rig root"), "{why}");
        // The distance is in the message — that is the number that says how bad it is.
        assert!(why.contains("91") || why.contains("92"), "should report the ~91.6 m distance: {why}");
    }

    #[test]
    fn non_finite_transforms_are_caught_before_the_distance_test() {
        // NaN fails every comparison, so a distance check alone would silently pass it. Order matters.
        let root = Vec3::ZERO;
        assert!(joint_fault(root, &at(f32::NAN, 0.0, 0.0)).is_some_and(|w| w.contains("non-finite")));
        assert!(joint_fault(root, &at(f32::INFINITY, 0.0, 0.0)).is_some_and(|w| w.contains("non-finite")));
    }

    #[test]
    fn a_degenerate_scale_is_caught_even_at_the_origin() {
        // A joint that never moved but whose scale blew up still stretches every vertex bound to it.
        let root = Vec3::ZERO;
        let huge = GlobalTransform::from(
            Transform::from_xyz(0.0, 1.0, 0.0).with_scale(Vec3::splat(5000.0)),
        );
        assert!(joint_fault(root, &huge).is_some_and(|w| w.contains("scale")));
    }

    #[test]
    fn the_limit_is_far_outside_any_pose_the_rig_can_reach() {
        // Guards the threshold itself: the figurine is ~1.82 m, so 8 m is >4x its full height. If
        // someone tightens this, a raised arm must still not trip it.
        assert!(MAX_JOINT_RADIUS > 1.82 * 4.0);
    }
}
