//! **Asset contract: the VALKYRIE glb still matches what `squad` was wired against.**
//!
//! GPU-free and `App`-free — this runs in the `cargo test` hard gate, so it blocks on every push.
//!
//! # Why this exists
//!
//! `squad::build_valkyrie_anim` addresses clips by *index* (`GltfAssetLabel::Animation(11)` is the
//! run), and `squad::drive_valkyrie_animation` reparameterises the gait clips onto a shared phase using
//! **baked** durations, phase offsets and cycle distances. Both are silent contracts with a binary
//! asset:
//!
//! * The Mixamo rifle retarget has already reordered these indices once. A second reorder would leave
//!   units sprinting with the reload clip and nothing would fail — it would just look wrong.
//! * The gait table's durations set the mapping from φ to seek time. Re-export a clip a few frames
//!   longer and every foot in the blend drifts out of sync, again silently.
//! * The upper-body mask is built by matching `squad::LOWER_BODY_BONES` against live bone names. Rename
//!   a bone and the aim/fire layer starts posing the legs.
//!
//! So the contract is asserted here, against the bytes, rather than left to a comment.

use std::collections::HashMap;

mod common;
use common::Glb;

const GLB: &str = "assets/characters/valkyrie.glb";

/// Clip index → name, for every clip `squad` wires. Mirrors the `CLIP_*` constants there.
const WIRED_CLIPS: [(usize, &str); 10] = [
    (0, "valkyrie_idle"),
    (1, "valkyrie_idle_alert"),
    (3, "valkyrie_aim"),
    (4, "valkyrie_fire"),
    (5, "valkyrie_walk"),
    (8, "valkyrie_walk_back"),
    (11, "valkyrie_run"),
    (12, "valkyrie_run_back"),
    // Wired by measured direction, not by name: clip 13 sidesteps to the character's RIGHT and clip 14
    // to its LEFT. See `squad::CLIP_STRAFE_LEFTWARD`.
    (13, "valkyrie_strafe_l"),
    (14, "valkyrie_strafe_r"),
];

/// Clip name → the duration baked into `squad`'s gait table, seconds. Tolerance is one 24 fps frame:
/// the table only has to be right to within a frame for the phase mapping to hold up.
const GAIT_DURATIONS: [(&str, f32); 6] = [
    ("valkyrie_walk", 1.417),
    ("valkyrie_run", 0.750),
    ("valkyrie_walk_back", 1.458),
    ("valkyrie_run_back", 0.583),
    ("valkyrie_strafe_l", 0.708),
    ("valkyrie_strafe_r", 0.583),
];

/// Every bone `squad::LOWER_BODY_BONES` puts in the upper-body mask group.
const LOWER_BODY_BONES: [&str; 14] = [
    "Root",
    "pelvis",
    "thigh_l",
    "thigh_r",
    "calf_l",
    "calf_r",
    "foot_l",
    "foot_r",
    "ball_l",
    "ball_r",
    "skirt_l",
    "skirt_r",
    "thigh_holster",
    "ammo_pouch",
];

/// One 24 fps frame.
const FRAME: f32 = 1.0 / 24.0;

#[test]
fn wired_clip_indices_still_name_the_clips_squad_expects() {
    let glb = Glb::load(GLB);
    let anims = glb.animations();
    assert_eq!(anims.len(), 20, "the rig should still carry 20 clips, found {}", anims.len());

    for (index, expected) in WIRED_CLIPS {
        let got = anims
            .get(index)
            .unwrap_or_else(|| panic!("clip index {index} is past the end of the rig"))["name"]
            .as_str()
            .unwrap_or_else(|| panic!("clip index {index} has no name"));
        assert_eq!(
            got, expected,
            "clip index {index} is now `{got}`, but src/squad.rs wires it as `{expected}` — the glb was \
             re-exported with a different clip order. Update the CLIP_* constants (and the gait table) \
             to match, then re-measure the phase offsets."
        );
    }
}

#[test]
fn the_gait_table_durations_match_the_asset() {
    let glb = Glb::load(GLB);
    let by_name: HashMap<&str, f32> = glb
        .animations()
        .iter()
        .filter_map(|a| a["name"].as_str().map(|n| (n, glb.duration(a))))
        .collect();

    for (name, baked) in GAIT_DURATIONS {
        let actual = by_name
            .get(name)
            .copied()
            .unwrap_or_else(|| panic!("clip `{name}` is gone from the rig"));
        assert!(
            (actual - baked).abs() <= FRAME,
            "clip `{name}` is {actual:.3} s but src/squad.rs bakes {baked:.3} s — the shared gait phase \
             maps φ onto the wrong part of the clip, so the feet will drift out of sync. Re-measure the \
             gait table."
        );
    }
}

#[test]
fn every_masked_lower_body_bone_exists_in_the_rig() {
    let glb = Glb::load(GLB);
    let names = glb.node_names();
    let missing: Vec<&str> = LOWER_BODY_BONES
        .iter()
        .copied()
        .filter(|b| !names.contains(b))
        .collect();
    assert!(
        missing.is_empty(),
        "src/squad.rs masks these bones out of the aim/fire layer, but the rig has no such nodes: \
         {missing:?}. Unmatched names silently shrink the mask, and the upper-body layer would start \
         posing the legs."
    );
}

/// The whole phase-sync design assumes the clips carry no root motion — movement is `Transform`-driven
/// by `unit_movement`, and the clip supplies leg motion only (Game AI Pro 2 ch. 36 §36.2.5's in-place
/// case). A re-export that bakes root translation back in would double up with the sim's own movement.
#[test]
fn the_locomotion_clips_are_still_authored_in_place() {
    let glb = Glb::load(GLB);
    // The FULL-array node id (see `Glb::node_index`) — a `position()` in the name-filtered list would be a
    // different number the moment the rig gains an unnamed node before `Root`, no channel would match, and
    // this gate would pass without ever running its assertion.
    let root = glb.node_index("Root");

    // Every clip carries a `Root` translation *channel* — the exporter writes one per bone — but the
    // keys are all identical, so the root never actually moves. It is that displacement, not the
    // channel's existence, that has to stay zero.
    for (index, name) in WIRED_CLIPS {
        let anims = glb.animations();
        let samplers = anims[index]["samplers"].as_array().expect("samplers");
        for channel in anims[index]["channels"].as_array().expect("channels") {
            if channel["target"]["node"].as_u64() != Some(root)
                || channel["target"]["path"].as_str() != Some("translation")
            {
                continue;
            }
            let sampler = &samplers[channel["sampler"].as_u64().expect("sampler index") as usize];
            let keys = glb.read_vec3(sampler["output"].as_u64().expect("output accessor") as usize);
            let mut travel = 0.0f32;
            for axis in 0..3 {
                let lo = keys.iter().map(|k| k[axis]).fold(f32::INFINITY, f32::min);
                let hi = keys.iter().map(|k| k[axis]).fold(f32::NEG_INFINITY, f32::max);
                travel = travel.max(hi - lo);
            }
            assert!(
                travel < 1.0e-4,
                "clip `{name}` (index {index}) moves `Root` by {travel} over its length. These clips \
                 must stay in-place: `unit_movement` already drives the character's `Transform`, so \
                 baked root motion would move it twice — and the shared gait phase derives its cadence \
                 from a measured cycle distance that assumes none."
            );
        }
    }
}
