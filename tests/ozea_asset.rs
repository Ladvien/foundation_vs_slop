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
    assert!(
        !scenes.is_empty(),
        "no scenes: every spawn site asks for Scene(0)"
    );
    let roots = scenes[0]["nodes"]
        .as_array()
        .expect("scene 0 has a nodes array");
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
    let nodes = Glb::load(GLB).json["nodes"]
        .as_array()
        .expect("nodes")
        .clone();
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
    assert!(
        has_tangents,
        "every primitive must carry TANGENT — Bevy does not regenerate them"
    );

    // Scenery is static. §4's clip contract is for characters, and an empty/animated prop would put
    // clips in front of `anim::PoseBlender`, which is wired for rigs only.
    let anims = glb
        .json
        .get("animations")
        .and_then(|a| a.as_array())
        .map_or(0, |a| a.len());
    assert_eq!(
        anims, 0,
        "a static prop must ship no animation clips, found {anims}"
    );
}

/// **Every mesh in `assets/ozea/` is XZ-centred with its base at `y = 0`** (`docs/artist_guide.md`
/// §3 rule 7).
///
/// # Why this is a test and not a comment
///
/// Nothing in this repo verified an asset's ORIGIN until 2026-08-01, and the cost was eleven of the
/// sixteen Ozea meshes silently carrying whatever pivot their source pack happened to have — walls and
/// floors centre-origined, props base-origined, one piece neither. The packs disagree with each other:
/// `SM_Wall_Corner` in HS_002 is off-centre by 15 mm in *two* axes.
///
/// It was invisible because nothing fails when an origin is wrong. `site::kit::y_scale` is
/// `target / authored` applied about the entity origin, so a centre-origined 2.0 m wall asked to reach
/// `WALL_HEIGHT` grew half its gain DOWNWARD: `Y[-1.2, +1.2]`, half the wall underground and 1.17 m
/// standing against 2.4 m intended. No error, no warning — just a hub whose walls were too short and
/// whose corners would not square, which is how the player found it.
///
/// The sibling test above reads the same bounds and uses only `hi - lo`, discarding the minimum. The
/// minimum *is* the origin. That is the gap this closes.
///
/// Raw accessor bounds are the right measure here because the converter bakes the Y-up conversion into
/// the vertex data — every node in these files is identity, which `no_node_carries_a_compensating_scale`
/// half-covers and this relies on.
#[test]
fn every_ozea_mesh_is_base_origined_and_xz_centred() {
    // 5 mm. Comfortably tighter than the 15 mm the source packs actually drift by, and loose enough
    // for float noise through an FBX import and a glTF export.
    const TOL: f32 = 0.005;

    let mut checked = 0;
    let mut bad: Vec<String> = Vec::new();
    let dir = std::fs::read_dir("assets/ozea").expect("assets/ozea exists");
    let mut paths: Vec<std::path::PathBuf> = dir
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "glb"))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no .glb files under assets/ozea — did the directory move?"
    );

    for path in &paths {
        let name = path
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .to_string();
        let (lo, hi) = accessor_bounds(&Glb::load(path.to_str().expect("utf-8 path")));
        checked += 1;
        let centre_x = (lo[0] + hi[0]) * 0.5;
        let centre_z = (lo[2] + hi[2]) * 0.5;
        if lo[1].abs() > TOL {
            bad.push(format!(
                "{name}: base at y={:.4}, not 0 (spans {:.3}..{:.3})",
                lo[1], lo[1], hi[1]
            ));
        }
        if centre_x.abs() > TOL || centre_z.abs() > TOL {
            bad.push(format!(
                "{name}: XZ centre ({centre_x:.4}, {centre_z:.4}), not (0, 0)"
            ));
        }
    }

    assert!(
        bad.is_empty(),
        "{} of {checked} Ozea meshes break the origin convention. Re-convert with \
         `scripts/fbx_to_glb.py --reorigin-base` (see assets/ozea/README.md); without it a mesh keeps \
         whatever pivot its source pack had, and `site::kit::y_scale` will bury half of it:\n  {}",
        bad.len(),
        bad.join("\n  ")
    );
    // 19 = the 16 promoted by hand, plus `wall_header.glb` and `wall_leg.glb` (both cropped from the
    // wall — for the doorway's header course and a junction's leg) and `slab.glb` (the research
    // wing's examination bed). A floor, not an equality:
    // promoting another mesh should not fail this test, but a glob that silently stops seeing the
    // kit must.
    assert!(
        checked >= 19,
        "expected the whole kit, only saw {checked} meshes"
    );
}

/// The wall family is authored to its target height, so nothing is stretched at runtime.
///
/// `assets/config/config.ron` records why this matters in the dungeon's own words — a 1.2x vertical
/// scale "would stretch the panel detailing", which is why the Ozea *walls* were never promoted into
/// the dungeon furniture kit. The Site was doing exactly that until these were re-authored by
/// `scripts/ozea_wall_heights.py`: 1.2x on the walls, and **2.18x** on the column.
///
/// Asserted against the asset rather than the kit file so that re-exporting a 2.0 m wall and leaving
/// `kit_ozea.ron` claiming 2.40 fails here, rather than silently reintroducing the stretch.
#[test]
fn the_wall_family_is_authored_to_full_height() {
    // `dungeon::WALL_HEIGHT`, duplicated as a literal for the same reason `DOORWAY_HEIGHT` is above:
    // the point is that the ASSET is on the game's grid, so importing it would let both drift together.
    const WALL_HEIGHT: f32 = 2.4;

    for piece in [
        "wall.glb",
        "wall_corner.glb",
        "wall_window.glb",
        "column.glb",
    ] {
        let (lo, hi) = accessor_bounds(&Glb::load(&format!("assets/ozea/{piece}")));
        let height = hi[1] - lo[1];
        assert!(
            (height - WALL_HEIGHT).abs() < 0.01,
            "{piece} is {height:.3} m, not {WALL_HEIGHT} m — `site::kit::y_scale` would stretch it by \
             {:.2}x at runtime, which is the distortion these variants exist to remove. Re-run \
             scripts/ozea_wall_heights.py.",
            WALL_HEIGHT / height
        );
    }
}
