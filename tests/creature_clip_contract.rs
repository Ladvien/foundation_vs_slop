//! **Asset contract: the crab, SCP-150 and SCP-1048 glbs still match what their wiring was written
//! against.**
//!
//! GPU-free and `App`-free — runs in the `cargo test` hard gate, so it blocks on every push.
//!
//! `crab::build_crab_anim` and `parasite::build_manca_anim` address clips by *index*
//! (`GltfAssetLabel::Animation(i)`), a silent contract with a binary asset: export the model with the
//! clips in a different order and every state plays the wrong animation with nothing failing — the
//! exact drift `tests/valkyrie_asset.rs` pins for the VALKYRIE rig. It is not hypothetical here
//! either: the SCP-150 export orders its 12 clips **alphabetically**, not in the authoring-tool order
//! the wiring was first written against, and the manca shipped with its dormant huddle playing
//! `Attack1` until the indices were reconciled against the bytes (2026-07-24).

mod common;
use common::Glb;

const CRAB_GLB: &str = "assets/dimensional_crab/dimensional_crab.glb";
/// Clip index → name for every clip `crab::build_crab_anim` wires. Mirrors the loads there.
const CRAB_WIRED: [(usize, &str); 3] = [(0, "attack"), (1, "idle"), (2, "walk")];
const CRAB_CLIP_COUNT: usize = 3;

const MANCA_GLB: &str = "assets/scp150/scp-150.glb";
/// Clip index → name for every clip `parasite::build_manca_anim` wires (of the 12 the glb stores
/// alphabetically). Mirrors the `Animation(i)` loads there.
const MANCA_WIRED: [(usize, &str); 6] = [
    (7, "Idle_Snug"),
    (6, "Idle_Alert"),
    (10, "Walk1"),
    (3, "Climb"),
    (1, "Attack2"),
    (2, "BurrowOut"),
];
const MANCA_CLIP_COUNT: usize = 12;

// ── The SCP-1048 family ───────────────────────────────────────────────────────────────────────────
//
// Four bears share one 8-bone rig and one clip vocabulary, but NOT one clip *order*: the benign
// original ships `draw_picture` (dropped from all three copies as tonally wrong) and each copy adds
// its own hostile set, so the same clip sits at a different index in each file. `slot_for` in
// `scp1048::anim` therefore carries a per-variant table, and every one of those tables is pinned
// here. Clip names are variant-prefixed (`scp1048_*`, `scp1048a_*`, …) precisely so all four can be
// loaded simultaneously without animation-name collisions.

const SCP1048_GLB: &str = "assets/scp1048/scp-1048.glb";
/// Clip index → name for the benign original. Every clip is wired.
const SCP1048_WIRED: [(usize, &str); 5] = [
    (0, "scp1048_rest_idle"),
    (1, "scp1048_dance"),
    (2, "scp1048_jump_in_place"),
    (3, "scp1048_draw_picture"),
    (4, "scp1048_sit_down"),
];
const SCP1048_CLIP_COUNT: usize = 5;

const SCP1048A_GLB: &str = "assets/scp1048a/scp-1048-a.glb";
/// Clip index → name for SCP-1048-A (the ear bear). Every clip is wired; `scream` is its attack.
const SCP1048A_WIRED: [(usize, &str); 5] = [
    (0, "scp1048a_rest_idle"),
    (1, "scp1048a_jump_in_place"),
    (2, "scp1048a_sit_down"),
    (3, "scp1048a_scream"),
    (4, "scp1048a_rage"),
];
const SCP1048A_CLIP_COUNT: usize = 5;

const SCP1048B_GLB: &str = "assets/scp1048b/scp-1048-b.glb";
/// Clip index → name for SCP-1048-B (the infant-arm bear). Every clip is wired; `tantrum` is its
/// attack and, unlike every other creature attack in this codebase, it **loops** — so it is driven
/// as a state, not triggered as an event.
const SCP1048B_WIRED: [(usize, &str); 6] = [
    (0, "scp1048b_rest_idle"),
    (1, "scp1048b_dance"),
    (2, "scp1048b_jump_in_place"),
    (3, "scp1048b_sit_down"),
    (4, "scp1048b_tantrum"),
    (5, "scp1048b_rage"),
];
const SCP1048B_CLIP_COUNT: usize = 6;

const SCP1048C_GLB: &str = "assets/scp1048c/scp-1048-c.glb";
/// Clip index → name for SCP-1048-C (the rusted scrap bear with the arm gun).
///
/// `scp1048c_dance` (index 1) is pinned but **deliberately left unwired** — it ships as legacy
/// motion inherited from the benign original and reads wrong on a violent copy (the asset's own
/// tonal note says so). Pinning it anyway keeps the gap documented rather than accidental: if a
/// re-export drops `dance`, the indices of the four hostile clips above it all shift, and this
/// assertion is what catches that.
const SCP1048C_WIRED: [(usize, &str); 8] = [
    (0, "scp1048c_rest_idle"),
    (1, "scp1048c_dance"),
    (2, "scp1048c_jump_in_place"),
    (3, "scp1048c_sit_down"),
    (4, "scp1048c_aim_gun"),
    (5, "scp1048c_fire_gun"),
    (6, "scp1048c_pistol_whip"),
    (7, "scp1048c_rage"),
];
const SCP1048C_CLIP_COUNT: usize = 8;

fn assert_wired(path: &str, clip_count: usize, wired: &[(usize, &str)], code_ref: &str) {
    let glb = Glb::load(path);
    let anims = glb.animations();
    assert_eq!(
        anims.len(),
        clip_count,
        "{path} should still carry {clip_count} clips, found {} — a re-export changed the set; \
         re-check every Animation(i) index in {code_ref}",
        anims.len()
    );
    for &(index, expected) in wired {
        let got = anims
            .get(index)
            .unwrap_or_else(|| panic!("clip index {index} is past the end of {path}"))["name"]
            .as_str()
            .unwrap_or_else(|| panic!("clip index {index} in {path} has no name"));
        assert_eq!(
            got, expected,
            "clip index {index} of {path} is now `{got}`, but {code_ref} wires it as `{expected}` — \
             the glb was re-exported with a different clip order. Update the Animation(i) indices \
             there (and the clip table in docs/artist_guide.md) to match."
        );
    }
}

#[test]
fn crab_clip_indices_still_name_the_clips_the_wiring_expects() {
    assert_wired(CRAB_GLB, CRAB_CLIP_COUNT, &CRAB_WIRED, "crab::build_crab_anim");
}

#[test]
fn manca_clip_indices_still_name_the_clips_the_wiring_expects() {
    assert_wired(MANCA_GLB, MANCA_CLIP_COUNT, &MANCA_WIRED, "parasite::build_manca_anim");
}

#[test]
fn scp1048_clip_indices_still_name_the_clips_the_wiring_expects() {
    assert_wired(SCP1048_GLB, SCP1048_CLIP_COUNT, &SCP1048_WIRED, "scp1048::anim::slot_for");
}

#[test]
fn scp1048a_clip_indices_still_name_the_clips_the_wiring_expects() {
    assert_wired(SCP1048A_GLB, SCP1048A_CLIP_COUNT, &SCP1048A_WIRED, "scp1048::anim::slot_for");
}

#[test]
fn scp1048b_clip_indices_still_name_the_clips_the_wiring_expects() {
    assert_wired(SCP1048B_GLB, SCP1048B_CLIP_COUNT, &SCP1048B_WIRED, "scp1048::anim::slot_for");
}

#[test]
fn scp1048c_clip_indices_still_name_the_clips_the_wiring_expects() {
    assert_wired(SCP1048C_GLB, SCP1048C_CLIP_COUNT, &SCP1048C_WIRED, "scp1048::anim::slot_for");
}

/// The four bears share a rig but not a clip *order*, and three of them share clip *names* modulo
/// the variant prefix — so a copy-paste slip in the tables above (pointing two variants at the same
/// glb, or reusing A's index list for B) would still pass `assert_wired` on the file it named while
/// silently leaving a variant unpinned. This holds that line: every wired name must carry its own
/// variant's prefix, and the four files must be four distinct paths.
#[test]
fn each_bear_variants_clips_carry_its_own_prefix() {
    let families: [(&str, &str, &[(usize, &str)]); 4] = [
        (SCP1048_GLB, "scp1048_", &SCP1048_WIRED),
        (SCP1048A_GLB, "scp1048a_", &SCP1048A_WIRED),
        (SCP1048B_GLB, "scp1048b_", &SCP1048B_WIRED),
        (SCP1048C_GLB, "scp1048c_", &SCP1048C_WIRED),
    ];
    for (path, prefix, wired) in families {
        for &(index, name) in wired {
            assert!(
                name.starts_with(prefix),
                "{path} clip {index} is wired as `{name}`, which does not start with `{prefix}` — \
                 the variant tables were crossed. Clip names are variant-prefixed precisely so all \
                 four bears can be loaded at once without animation-name collisions."
            );
        }
    }
    let paths = [SCP1048_GLB, SCP1048A_GLB, SCP1048B_GLB, SCP1048C_GLB];
    for (i, a) in paths.iter().enumerate() {
        for b in &paths[i + 1..] {
            assert_ne!(a, b, "two bear variants point at the same glb — one of them is unpinned");
        }
    }
}
