//! **Asset contract: the Site-67 staff rigs still ship what `site::staff_anim` was wired against.**
//!
//! GPU-free and `App`-free — this runs in the `cargo test` hard gate, so it blocks on every push.
//! `tests/valkyrie_asset.rs` is the same idea for the squad figurine; read its header first.
//!
//! # Why this exists, and why it is more exposed than the Valkyrie's
//!
//! The `game-ready` skill names the trap directly: **only VALKYRIE has a build entry point.**
//! `researcher`, `scientist`, `fieldop`, `makarov` and the four `cipher_*` outfits ship `.glb`s with no
//! script behind them, so the failure that skill records — *"a hand-typed re-export once omitted
//! `stage.arm_mocap_shooter()` and silently shipped 8 animations instead of 20"* — is live and
//! otherwise unguarded for every rig here.
//!
//! Three silent contracts, all with a binary asset nobody in this repo builds:
//!
//! 1. **One shared clip vocabulary, in one order.** Measured 2026-08-02: all eight rigs carry the same
//!    20 clips at the same indices. That is what lets `site::staff_anim` hold a *single* `ClipSpec`
//!    table for the whole cast rather than one per rig (the `src/scp1048/anim.rs` shape, minus its
//!    per-variant `TABLES` array). If one rig is re-exported with a different order, every staff
//!    member on it plays the wrong clip and **nothing fails** — a cook would sprint on the spot.
//! 2. **One rig shape.** 55 joints, one skin, at most four influences per vertex. The slot table and
//!    the graph builder assume it.
//! 3. **The textures stay downscaled.** Four of these rigs arrived at 42–44 MB, of which 39.4 MB was a
//!    *shared* 4K `Fabric019` PBR set (a 32.7 MB normal map alone). They were rewritten in place by
//!    `scripts/glb_recompress_texture.py` — container only, geometry and animation bytes copied
//!    through untouched. A well-meaning re-copy from `scp_characters/gltf/` reinstates 160 MB of
//!    texture across the cast and nothing fails; the game just loads slower forever.

mod common;
use common::Glb;

/// Every rig the Site-67 cast is drawn from, as `assets/` sees it.
const STAFF_RIGS: [&str; 8] = [
    "researcher",
    "scientist",
    "fieldop",
    "makarov",
    "cipher_standard",
    "cipher_senior",
    "cipher_field",
    "cipher_hazmat",
];

/// **The shared clip vocabulary, in shipped index order.** Measured across all eight rigs on
/// 2026-08-02 with `scp_characters/scripts/inspect_glb.py`; every one of them matched exactly.
///
/// Clip names are `<prefix>_<suffix>`, where the prefix is the rig family (`cipher` for all four
/// outfits, since they are one identity re-dressed). This table is the suffix half — the half
/// `site::staff_anim` addresses by index.
const CLIP_VOCABULARY: [&str; 20] = [
    "idle",
    "idle_alert",
    "idle_look",
    "crouch_idle",
    "walk",
    "walk_back",
    "jog",
    "jog_back",
    "run",
    "sprint",
    "crouch_walk",
    "sneak",
    "jump",
    "wave",
    "salute",
    "point",
    "cheer",
    "sit",
    "hit_react",
    "death",
];

/// Joint count shared by every staff rig (the MPFB2 `game_engine` skeleton these were built on).
const JOINTS: usize = 55;

/// Per-rig triangle ceiling.
///
/// ⚠️ **These rigs are UNDECIMATED**, unlike the Valkyrie (82,436 → 5,225, a 15.6× cut). That is a
/// deliberately open decision, not an oversight: the plan defers it behind an A-B-A frame-cost
/// measurement rather than paying for a Blender pipeline these rigs have no entry point for. So this
/// ceiling is not "the budget" — it is a ratchet that stops them growing while that decision is open,
/// and it will drop sharply if the measurement says decimate.
///
/// Measured 2026-08-02: researcher 37,156 · scientist 36,272 · fieldop 36,988 · makarov 38,760 ·
/// cipher_senior 73,676 · cipher_field 79,520 · cipher_standard 86,920 · cipher_hazmat 96,344.
///
/// Note the four `cipher_*` outfits are 2–2.6× the plain archetypes, and that the asset project's own
/// `README.md` reports cipher_standard at 57,980 — **stale by 29,000 triangles**. Measure, never quote.
const TRIANGLE_CEILING: usize = 100_000;

/// Ceiling on total embedded image bytes per rig.
///
/// The four heavy rigs each carried the same three 4K maps: `Fabric019_4K-JPG_NormalGL` 32,690,253 B,
/// `_Color` 6,000,938 B, `_Roughness` ~744,986 B. Downscaled to 512/1024/512 they total ~544 KB, and
/// the rigs that never had them carry only a 64×128 hair pair (~6–9 KB). 1.5 MB is comfortably above
/// the former and three orders of magnitude below the latter, so this fires on exactly one mistake:
/// a rig re-copied from the asset project without the recompression pass.
const IMAGE_BYTES_CEILING: usize = 1_500_000;

fn path(rig: &str) -> String {
    format!("assets/characters/{rig}.glb")
}

/// The clip-name prefix a rig uses, derived from its own first clip rather than hardcoded — the four
/// `cipher_*` outfits all use the bare prefix `cipher`, and hardcoding that mapping would be a second
/// place for it to be wrong.
fn prefix_of(first_clip: &str) -> &str {
    first_clip
        .strip_suffix("_idle")
        .unwrap_or_else(|| panic!("clip 0 is `{first_clip}`, which is not `<prefix>_idle`"))
}

#[test]
fn every_staff_rig_ships_the_same_twenty_clips_in_the_same_order() {
    // **The dangerous half of the contract.** `inspect_glb.py --baseline` says it outright: every name
    // can match while the indices shuffle. `site::staff_anim` loads by index, so a reorder is invisible
    // until someone notices the archivist saluting instead of sitting.
    for rig in STAFF_RIGS {
        let p = path(rig);
        let glb = Glb::load(&p);
        let anims = glb.animations();
        assert_eq!(
            anims.len(),
            CLIP_VOCABULARY.len(),
            "{p} ships {} clips, not {}. The `game-ready` skill records exactly this failure: a \
             re-export that omits a mocap stage silently ships 8 clips instead of 20, and only \
             VALKYRIE has a build script that asserts the count before writing.",
            anims.len(),
            CLIP_VOCABULARY.len()
        );

        let first = anims[0]["name"].as_str().unwrap_or_else(|| panic!("{p} clip 0 has no name"));
        let prefix = prefix_of(first);

        for (index, suffix) in CLIP_VOCABULARY.iter().enumerate() {
            let got = anims[index]["name"]
                .as_str()
                .unwrap_or_else(|| panic!("{p} clip {index} has no name"));
            let expected = format!("{prefix}_{suffix}");
            assert_eq!(
                got, expected,
                "{p} clip index {index} is now `{got}`, but the shared vocabulary puts `{expected}` \
                 there. Either this rig was re-exported with a different clip order — in which case \
                 `site::staff_anim`'s single ClipSpec table no longer serves the whole cast — or the \
                 vocabulary itself changed and CLIP_VOCABULARY needs updating first."
            );
        }
    }
}

#[test]
fn every_staff_rig_has_the_one_skeleton_the_slot_table_assumes() {
    // One shared clip table only works over one shared rig shape. Checked per rig rather than
    // rig-against-rig so the failure names the file that drifted.
    for rig in STAFF_RIGS {
        let p = path(rig);
        let glb = Glb::load(&p);

        let skins = glb.json["skins"].as_array().unwrap_or_else(|| panic!("{p} has no skins array"));
        assert_eq!(skins.len(), 1, "{p} has {} skins; the staff path binds exactly one", skins.len());

        let joints = skins[0]["joints"].as_array().unwrap_or_else(|| panic!("{p} skin has no joints"));
        assert_eq!(
            joints.len(),
            JOINTS,
            "{p} has {} joints, not {JOINTS} — the staff rigs share the MPFB2 `game_engine` skeleton, \
             and a different one means a different retarget",
            joints.len()
        );

        // glTF allows four influences per JOINTS_n set; a second set means five or more, which the
        // `game-ready` skill's pipeline exists to re-cap and which Bevy will silently truncate.
        for mesh in glb.json["meshes"].as_array().unwrap_or_else(|| panic!("{p} has no meshes")) {
            let name = mesh["name"].as_str().unwrap_or("<unnamed>");
            for prim in mesh["primitives"].as_array().unwrap_or_else(|| panic!("{p} mesh has no primitives")) {
                assert!(
                    prim["attributes"]["JOINTS_1"].is_null(),
                    "{p} mesh `{name}` carries JOINTS_1 — more than 4 bone influences per vertex. \
                     Bevy truncates silently; re-cap influences to 4 before shipping."
                );
            }
        }
    }
}

#[test]
fn the_staff_rigs_keep_their_downscaled_textures() {
    // The regression is a *helpful* one: someone re-copies a rig from `scp_characters/gltf/` to pick
    // up an art fix and silently reinstates 39.4 MB of 4K fabric per rig. Nothing fails — the game
    // just loads slower forever, and `assets/characters/` goes from 30 MB to ~160 MB.
    for rig in STAFF_RIGS {
        let p = path(rig);
        let glb = Glb::load(&p);
        let views = glb.json["bufferViews"].as_array().unwrap_or_else(|| panic!("{p} has no bufferViews"));

        let mut total = 0usize;
        let images = glb.json["images"].as_array().map(|v| v.as_slice()).unwrap_or(&[]);
        for image in images {
            let view = image["bufferView"]
                .as_u64()
                .unwrap_or_else(|| panic!("{p} has an image with no bufferView — external URIs do not ship"))
                as usize;
            total += views[view]["byteLength"].as_u64().expect("bufferView byteLength") as usize;
        }

        assert!(
            total <= IMAGE_BYTES_CEILING,
            "{p} carries {total} bytes of embedded texture, over the {IMAGE_BYTES_CEILING} ceiling. \
             Four of these rigs shipped upstream with a shared 4K Fabric019 set (39.4 MB each). Run \
             `scripts/glb_recompress_texture.py` on images 2/3/4 rather than copying the raw file."
        );
    }
}

#[test]
fn the_staff_rigs_do_not_grow_while_the_decimation_decision_is_open() {
    // See TRIANGLE_CEILING. This is a ratchet, not a budget — these rigs are undecimated on purpose
    // and the real number is pending an A-B-A frame-cost measurement.
    for rig in STAFF_RIGS {
        let p = path(rig);
        let glb = Glb::load(&p);
        let accessors = glb.json["accessors"].as_array().unwrap_or_else(|| panic!("{p} has no accessors"));

        let mut tris = 0usize;
        for mesh in glb.json["meshes"].as_array().unwrap_or_else(|| panic!("{p} has no meshes")) {
            for prim in mesh["primitives"].as_array().unwrap_or_else(|| panic!("{p} mesh has no primitives")) {
                // Same fallback as `tests/valkyrie_asset.rs`: count POSITION for a non-indexed prim
                // rather than skipping it, so an unindexed re-export cannot slip the ceiling.
                let acc = prim["indices"]
                    .as_u64()
                    .or_else(|| prim["attributes"]["POSITION"].as_u64())
                    .unwrap_or_else(|| panic!("{p} has a primitive with neither indices nor POSITION"))
                    as usize;
                tris += accessors[acc]["count"].as_u64().expect("accessor count") as usize / 3;
            }
        }

        assert!(
            tris <= TRIANGLE_CEILING,
            "{p} is {tris} triangles, over the {TRIANGLE_CEILING} ratchet. These rigs are shipped \
             undecimated by an open decision — if this grew from an art change, decimate before \
             importing; if it grew because the ratchet was always too tight, measure the frame cost \
             before raising it."
        );
    }
}
