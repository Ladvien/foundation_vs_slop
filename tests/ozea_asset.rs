//! **Asset contract: the converted Ozea prop matches `docs/artist_guide.md` §3** (FVS-N-10).
//!
//! GPU-free and `App`-free — runs in the `cargo test` hard gate, so it blocks on every push.
//!
//! # Why this exists
//!
//! `scripts/fbx_to_glb.py` converts a library that ships **zero `.glb`** into the only format the
//! artist guide permits. A conversion script is a silent contract: it either honours §3 or it produces
//! assets that look wrong in ways nothing fails on. Three of the §3 rules are exactly that shape —
//! units, scene index, and tangents all degrade *visually* rather than loudly.
//!
//! # The bug this pins, which was real
//!
//! The FBX importer represents this library's centimetre authoring as a node `scale` of `0.01` over
//! **100× vertex data**. That is valid glTF and Bevy renders it correctly, because the node transform
//! applies. But it leaves a trap: anything reading a mesh AABB *directly* — a collider, a bbox check, a
//! placement heuristic — sees centimetres and is wrong by 100× with no error anywhere.
//!
//! Measured on this exact prop before the fix: accessor bounds **200.3 units**, node scale `0.01`, true
//! height `2.003 m`. The converter now bakes the transform into the mesh data, so "the numbers in the
//! file are metres" is true rather than true-after-a-multiplication — and this test reads the raw
//! accessor bounds specifically so it would catch a regression to the old shape.

mod common;
use common::Glb;

const GLB: &str = "assets/ozea/doorframe_double.glb";

/// `dungeon::DOORWAY_HEIGHT`. Duplicated as a literal on purpose: the point of the test is that the
/// *asset* is on the game's grid, so importing the constant would let both drift together.
const DOORWAY_HEIGHT: f32 = 2.0;
/// `dungeon::TILE_SIZE`.
const TILE_SIZE: f32 = 1.0;

/// Axis-aligned bounds over every mesh primitive's `POSITION` accessor, in the file's own units.
fn accessor_bounds(glb: &Glb) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    let accessors = glb.json["accessors"].as_array().expect("accessors");
    for mesh in glb.json["meshes"].as_array().expect("meshes") {
        for prim in mesh["primitives"].as_array().expect("primitives") {
            let idx = prim["attributes"]["POSITION"].as_u64().expect("POSITION") as usize;
            let a = &accessors[idx];
            for i in 0..3 {
                lo[i] = lo[i].min(a["min"][i].as_f64().expect("min") as f32);
                hi[i] = hi[i].max(a["max"][i].as_f64().expect("max") as f32);
            }
        }
    }
    (lo, hi)
}

#[test]
fn the_converted_prop_is_a_binary_gltf_with_the_asset_in_scene_zero() {
    // §3 rules 1 and 3. `Glb::load` already refuses anything that is not a well-formed binary glTF
    // container, so reaching this line covers rule 1.
    let glb = Glb::load(GLB);
    let scenes = glb.json["scenes"].as_array().expect("at least one scene");
    assert!(!scenes.is_empty(), "no scenes: every spawn site asks for Scene(0)");
    let roots = scenes[0]["nodes"].as_array().expect("scene 0 has a nodes array");
    assert!(
        !roots.is_empty(),
        "scene 0 is EMPTY — every spawn site calls GltfAssetLabel::Scene(0), so the asset would load \
         as nothing at all rather than fail"
    );
}

#[test]
fn the_prop_is_authored_in_metres_on_the_games_grid() {
    // §3 rule 2, and the 100x trap in the module docs. Read from the RAW accessor bounds rather than
    // from a resolved world transform, precisely so a regression to "cm data + 0.01 node scale" fails
    // here instead of surfacing as a collider that does not match what is drawn.
    let glb = Glb::load(GLB);
    let (lo, hi) = accessor_bounds(&glb);
    let dims = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];

    assert!(
        (dims[1] - DOORWAY_HEIGHT).abs() < 0.1,
        "height {} m is not a doorway ({DOORWAY_HEIGHT} m). If this is ~200, the converter stopped \
         baking the object scale and the mesh data is back in CENTIMETRES: {dims:?}",
        dims[1]
    );
    // A double doorway spans two tiles. Loose, because the frame carries trim beyond the opening.
    assert!(
        (dims[2] - 2.0 * TILE_SIZE).abs() < 0.25,
        "width {} m does not span two {TILE_SIZE} m tiles: {dims:?}",
        dims[2]
    );
    assert!(
        dims.iter().all(|d| *d > 0.01 && *d < 12.0),
        "implausible extents for a prop — artist guide §3 rule 6 exists because a 10 m shelf once \
         forced a kit swap: {dims:?}"
    );
}

#[test]
fn no_node_carries_a_compensating_scale() {
    // The other half of the units check, stated directly. A node scale is not *wrong* in glTF — it is
    // how the importer expressed the cm authoring — but it is the thing that makes the accessor bounds
    // lie, so the converter removes it and this makes that permanent.
    let nodes = Glb::load(GLB).json["nodes"].as_array().expect("nodes").clone();
    for (i, n) in nodes.iter().enumerate() {
        if let Some(scale) = n.get("scale").and_then(|s| s.as_array()) {
            for (axis, v) in scale.iter().enumerate() {
                let v = v.as_f64().expect("scale component") as f32;
                assert!(
                    (v - 1.0).abs() < 1.0e-3,
                    "node {i} axis {axis} carries scale {v}: the mesh data is not in metres by itself"
                );
            }
        }
        assert!(
            n.get("matrix").is_none(),
            "node {i} carries a full matrix — the units check reads accessor bounds and cannot see it"
        );
    }
}

#[test]
fn the_prop_ships_tangents_and_no_animations() {
    let glb = Glb::load(GLB);
    // "Strongly preferred" in §3: Bevy does not regenerate tangents, so a normal-mapped prop without
    // them lights wrongly and silently.
    let has_tangents = glb.json["meshes"]
        .as_array()
        .expect("meshes")
        .iter()
        .flat_map(|m| m["primitives"].as_array().expect("primitives"))
        .all(|p| p["attributes"].get("TANGENT").is_some());
    assert!(has_tangents, "every primitive must carry TANGENT — Bevy does not regenerate them");

    // Scenery is static. §4's clip contract is for characters, and an empty/animated prop would put
    // clips in front of `anim::PoseBlender`, which is wired for rigs only.
    let anims = glb.json.get("animations").and_then(|a| a.as_array()).map_or(0, |a| a.len());
    assert_eq!(anims, 0, "a static prop must ship no animation clips, found {anims}");
}
