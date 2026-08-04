//! **The manifest agrees with the assets it describes.**
//!
//! `assets/emerge/rigs.ron` carries numbers that were measured off the GLBs. Before
//! `emerge_core::clips` existed there was no way to re-check them, so `docs/animation.md` recorded the
//! measuring as *"a manual offline step, not a repo tool"* and the numbers quietly aged: an artist
//! re-exports a rig, the clip order or a cycle length shifts, and the game keeps animating to the old
//! table with no error anywhere — a creature that skates or drifts out of phase, which reads as "the
//! animation feels bad" rather than as a stale constant.
//!
//! This is the check that closes that. It re-measures every gait in the manifest from the file the
//! manifest names, and fails when they part company.

use std::path::{Path, PathBuf};

use emerge_core::clips;
use emerge_core::glb::Glb;
use emerge_core::rigs::{Playback, Rigs};

/// The workspace root — tests run with the crate directory as cwd.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("emerge-core should sit two levels below the workspace root"))
        .to_path_buf()
}

fn manifest() -> Rigs {
    let path = root().join("assets/emerge/rigs.ron");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    Rigs::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_manifest_is_valid() {
    let rigs = manifest();
    assert!(!rigs.rigs.is_empty(), "the manifest describes no rigs");
}

/// Every clip a rig names exists in the GLB, and its duration is the asset's duration.
///
/// **Duration is the tripwire for a re-export.** Clip indices are positional, so a rig exported with
/// one extra animation shifts every index after it — and the symptom is a creature playing the wrong
/// clip, not an error. A duration that no longer matches is that shift showing up as a number.
#[test]
fn every_clip_exists_and_has_the_duration_the_manifest_claims() {
    const FRAME: f32 = 1.0 / 24.0;
    for (name, rig) in &manifest().rigs {
        let path = root().join("assets").join(&rig.mesh);
        let glb = Glb::open(&path).unwrap_or_else(|e| panic!("rig `{name}`: {}: {e}", path.display()));
        let found = clips::clips(&glb);
        for (i, slot) in rig.slots.iter().enumerate() {
            let c = found.get(slot.clip).unwrap_or_else(|| {
                panic!(
                    "rig `{name}` slot {i} names clip {} but {} has only {} clips — the asset was \
                     re-exported and the manifest was not re-measured",
                    slot.clip,
                    rig.mesh,
                    found.len()
                )
            });
            if let Playback::Gait { duration, .. } = slot.playback {
                assert!(
                    (c.duration - duration).abs() < FRAME,
                    "rig `{name}` slot {i} (clip {}, {:?}) is {:.3}s in the asset but {duration:.3}s \
                     in the manifest — more than a frame apart",
                    slot.clip,
                    c.name,
                    c.duration
                );
            }
        }
    }
}

/// The gait clips are still authored in place, which is what makes `cycle_distance` meaningful at all.
#[test]
fn gait_clips_carry_no_root_motion() {
    for (name, rig) in &manifest().rigs {
        let path = root().join("assets").join(&rig.mesh);
        let glb = Glb::open(&path).unwrap_or_else(|e| panic!("rig `{name}`: {e}"));
        let Some(root_node) = clips::node_index(&glb, "Root") else {
            continue;
        };
        for slot in &rig.slots {
            if !matches!(slot.playback, Playback::Gait { .. }) {
                continue;
            }
            let m = clips::root_motion(&glb, slot.clip, root_node);
            for (axis, v) in m.iter().enumerate() {
                assert!(
                    *v < 1.0e-4,
                    "rig `{name}` clip {} moves Root on axis {axis} by {v} — a gait clip must be \
                     authored in place; the game drives the transform itself",
                    slot.clip
                );
            }
        }
    }
}

/// **The measured cycle distance still agrees with the manifest's.**
///
/// Loose on purpose, and the looseness is honest rather than convenient. `docs/artist_guide.md` §4
/// says the set's phase offsets "agree to within 0.14 of a cycle (walk and run to within 0.016)" —
/// the hand-measured back and strafe numbers are themselves rough, so a tight bound here would be
/// asserting their error rather than the asset's truth. 20% catches a re-export that changed a
/// stride, which is what this is for; `clips.rs`'s own test pins the reference gaits to 3%.
#[test]
fn measured_cycle_distances_have_not_drifted() {
    /// `squad::FIGURINE_SCALE` — the manifest records world units, the GLB is in file units.
    const FIGURINE_SCALE: f32 = 1.13;
    for (name, rig) in &manifest().rigs {
        let path = root().join("assets").join(&rig.mesh);
        let glb = Glb::open(&path).unwrap_or_else(|e| panic!("rig `{name}`: {e}"));
        let Some(foot) = clips::node_index(&glb, "foot_l") else {
            continue;
        };
        for (i, slot) in rig.slots.iter().enumerate() {
            let Playback::Gait { cycle_distance, .. } = slot.playback else {
                continue;
            };
            let Some(raw) = clips::cycle_distance(&glb, slot.clip, foot) else {
                panic!("rig `{name}` slot {i} (clip {}) has no measurable stance", slot.clip);
            };
            let measured = raw * FIGURINE_SCALE;
            let err = (measured - cycle_distance).abs() / cycle_distance;
            assert!(
                err < 0.20,
                "rig `{name}` slot {i} (clip {}) measures {measured:.3} u/cycle but the manifest \
                 says {cycle_distance:.3} ({:.0}% out). Re-measure with \
                 `emerge_core::clips::cycle_distance` and update assets/emerge/rigs.ron.",
                slot.clip,
                err * 100.0
            );
        }
    }
}
