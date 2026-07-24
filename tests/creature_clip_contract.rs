//! **Asset contract: the crab and SCP-150 glbs still match what their wiring was written against.**
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
