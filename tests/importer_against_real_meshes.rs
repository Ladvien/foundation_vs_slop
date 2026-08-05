//! **The importer, pointed at meshes that actually exist.**
//!
//! `emerge_core::import`'s unit tests use hand-built measurements, which prove the rules and prove
//! nothing about the assets. This runs the real scanner over the shipped kits — the same call the
//! editor's import mode makes — and asserts the things that would make it useless: findings that fire
//! on clean assets, or a scan that cannot read the files this project ships.
//!
//! It also prints the report, because the output being *readable* is most of the feature and a test
//! that never shows it cannot tell anyone whether it is.

use std::path::Path;

use emerge_core::import::{self, Severity};
use emerge_core::library::Library;

fn library() -> Library {
    let text = std::fs::read_to_string("assets/emerge/library.ron")
        .unwrap_or_else(|e| panic!("assets/emerge/library.ron: {e}"));
    Library::parse(&text).unwrap_or_else(|e| panic!("{e}"))
}

/// The Ozea kit is this project's best-behaved set — `tests/ozea_asset.rs` pins it base-at-origin and
/// XZ-centred to 5 mm — so it is the right thing to point a warning system at first.
///
/// It raises **exactly one** warning today, and that warning is true: `vending_machine.glb` is 70,286
/// triangles against a kit median of 1,526 and a second-densest of 7,996. It is 46x the median and 9x
/// anything else in the set, which is what a decimation check is for.
///
/// So this pins the known state rather than demanding silence. A NEW warning fails it — that is the
/// point — and so does the vending machine being fixed, which should be noticed rather than quietly
/// making a test looser.
#[test]
fn the_ozea_kit_raises_exactly_the_one_warning_we_know_about() {
    let root = Path::new("assets");
    let candidates = import::scan(root, &root.join("ozea"), &library())
        .unwrap_or_else(|e| panic!("scan: {e}"));
    assert!(
        candidates.len() >= 18,
        "expected the whole Ozea kit, saw {}",
        candidates.len()
    );

    let loud: Vec<String> = candidates
        .iter()
        .flat_map(|c| {
            c.findings
                .iter()
                .filter(|f| f.severity >= Severity::Warn)
                .map(move |f| format!("{}: {}", c.mesh, f.message))
        })
        .collect();

    assert_eq!(
        loud.len(),
        1,
        "expected exactly the known vending-machine density warning, got {}:\n  {}",
        loud.len(),
        loud.join("\n  ")
    );
    assert!(
        loud[0].contains("vending_machine") && loud[0].contains("triangles"),
        "the one warning changed: {}",
        loud[0]
    );
}

/// **Every shipped mesh is measurable**, and getting there is the story of this file.
///
/// It first passed while checking three directories, and the editor — which walks the whole tree —
/// reported one unmeasurable mesh. Widened, it named `low_poly_flashlight.glb`, whose non-uniform
/// node scales made its accessor bounds meaningless.
///
/// The response was almost to pin that as a known exception. The real answer was that
/// `Glb::bounds` should compose the scene graph instead of reading accessors alone — which it now
/// does, so the flashlight measures correctly and so does every multi-part kit in the tree. A
/// `Blocking` finding here means the importer refuses an asset the game already loads, and that is
/// the importer being broken rather than strict.
#[test]
fn nothing_shipped_is_unmeasurable() {
    // **The whole tree, not three directories.** The first version checked `ozea`,
    // `low_poly_furniture` and `kenney_prototype-kit` and passed — and the editor's own scan, which
    // walks everything under `assets/`, reported one unmeasurable mesh. A test that checks a subset
    // of what the tool does is a test that agrees with the tool right up until it matters.
    let root = Path::new("assets");
    let mut blocked = Vec::new();
    for c in import::scan(root, root, &library()).unwrap_or_else(|e| panic!("scan: {e}")) {
        if c.blocked() {
            let why: Vec<&str> = c
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Blocking)
                .map(|f| f.message.as_str())
                .collect();
            blocked.push(format!("{}: {}", c.mesh, why.join("; ")));
        }
    }
    assert!(
        blocked.is_empty(),
        "{} shipped mesh(es) cannot be measured:\n  {}",
        blocked.len(),
        blocked.join("\n  ")
    );
}


/// The proposal has to be usable, not merely produced: a descriptor with no footprint reserves no
/// floor, and `check_prop_placements` would then let it overlap everything.
#[test]
fn every_proposal_carries_the_measurements_a_placement_needs() {
    let root = Path::new("assets");
    for c in import::scan(root, &root.join("ozea"), &library()).unwrap_or_else(|e| panic!("{e}")) {
        assert!(
            c.proposed.extent.footprint.is_some(),
            "{}: proposed without a footprint",
            c.mesh
        );
        assert!(c.proposed.extent.height.is_some(), "{}: no height", c.mesh);
        assert!(!c.proposed.id.is_empty(), "{}: no id", c.mesh);
        assert!(c.triangles > 0, "{}: no triangles counted", c.mesh);
    }
}

/// A mesh already in the library is not a candidate — otherwise the import list is every asset you
/// have ever imported, which is a list nobody reads twice.
#[test]
fn meshes_already_in_the_library_are_not_offered_again() {
    let root = Path::new("assets");
    let lib = library();
    let known: Vec<&str> = lib.descriptors.iter().filter_map(|d| d.mesh.as_deref()).collect();
    for c in import::scan(root, root, &lib).unwrap_or_else(|e| panic!("{e}")) {
        assert!(
            !known.contains(&c.mesh.as_str()),
            "{} is already in the library and was offered anyway",
            c.mesh
        );
    }
}

/// **An assembled model measures as it is assembled.**
///
/// `animal-horse.glb` is built from parts placed by node TRANSLATION — head lifted, legs spread —
/// and `Glb::bounds` read only the accessors, which describe those parts in their own space. It
/// therefore measured a pile at the origin. Nothing caught it until the import preview drew the mesh
/// standing through the top of its own volume box, which is what a wrong measurement looks like once
/// you finally render it.
///
/// The check is against the assembled extents rather than a pinned number, so it survives the asset
/// being re-exported: the horse is taller than the tallest single part it is made of.
#[test]
fn a_model_assembled_by_node_transforms_measures_as_assembled() {
    let path = Path::new("assets/kenney_prototype-kit/Models/GLB format/animal-horse.glb");
    let glb = emerge_core::glb::Glb::open(path).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        glb.has_node_transform(),
        "this asset is the fixture BECAUSE its parts are placed by node transform — if that stopped \
         being true, this test is measuring nothing"
    );

    let composed = glb.measure().unwrap_or_else(|e| panic!("{e}"));
    // The raw accessor union, which is what the old code returned: parts stacked at the origin.
    let raw_height = raw_accessor_height(path);
    assert!(
        composed.height > raw_height + 0.05,
        "composed height {:.3} m should exceed the raw accessor union {:.3} m — if they match, the \
         node transforms are not being composed",
        composed.height,
        raw_height
    );
}

/// The union of every POSITION accessor's declared bounds, ignoring the scene graph — the number
/// `Glb::bounds` used to return.
fn raw_accessor_height(path: &Path) -> f32 {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{e}"));
    let json_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let json: serde_json::Value =
        serde_json::from_slice(&bytes[20..20 + json_len]).unwrap_or_else(|e| panic!("{e}"));
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for mesh in json["meshes"].as_array().into_iter().flatten() {
        for prim in mesh["primitives"].as_array().into_iter().flatten() {
            let ix = prim["attributes"]["POSITION"].as_u64().unwrap_or(0) as usize;
            let acc = &json["accessors"][ix];
            lo = lo.min(acc["min"][1].as_f64().unwrap_or(0.0) as f32);
            hi = hi.max(acc["max"][1].as_f64().unwrap_or(0.0) as f32);
        }
    }
    hi - lo
}

/// **Occupancy, on meshes that exist** — the read behind the editor's "rescan mesh" button.
///
/// `emerge_core::import::occupancy`'s unit tests use hand-built triangles, which prove the
/// rasteriser and prove nothing about the assets. This runs it over the shipped architecture.
///
/// # What it reports, and why the numbers are not all "full"
///
/// Solid pieces come back completely marked, which is the point — a wall is solid all the way up:
///
/// ```text
/// site/wall             1 x 5 x 2    10 of 10
/// site/wall_corner      1 x 5 x 1     5 of  5
/// site/wall_window      1 x 5 x 4    20 of 20
/// site/column           1 x 5 x 2    10 of 10
/// site/wall_doorway_wide 1 x 4 x 4   12 of 16   <- the opening
/// site/slab             4 x 3 x 2    22 of 24   <- the notch
/// ```
///
/// The two that are *not* full are the check that this is measuring shape rather than answering
/// "yes" to everything: a wide doorway has cells that are entirely hole, and the slab has a notch.
///
/// **This replaced vertex occupancy**, which marked the cell each vertex fell in and therefore left
/// the middle of every low-poly slab open — `site/wall` came back 4 of 10, with only its top and
/// bottom layers marked, because a box has vertices only at its corners. That was the common case
/// for architecture, which is the half of a kit the lattice exists for.
#[test]
fn scanning_the_shipped_architecture_stays_in_range_and_records_its_coverage() {
    let layered = emerge_core::policy::layered_library(Path::new("assets/emerge/site"))
        .unwrap_or_else(|e| panic!("{e}"));

    let mut checked = 0usize;
    for d in &layered.library.descriptors {
        let Some(mesh) = d.mesh.as_deref() else { continue };
        let path = Path::new("assets").join(mesh);
        let Ok(glb) = emerge_core::glb::Glb::open(&path) else { continue };

        let div = emerge_core::descriptor::divisions(d, layered.policy.divisions)
            .unwrap_or_else(|e| panic!("{e}"));
        let total = emerge_core::descriptor::Subgrid::volume(div);
        let cells = emerge_core::import::occupancy(&glb, div, (0, 0, 0))
            .unwrap_or_else(|e| panic!("{}: {e}", d.id));

        assert!(!cells.is_empty(), "{} scanned to nothing", d.id);
        assert!(
            cells.len() as u32 <= total,
            "{} marked {} of {total} — a cell outside the lattice",
            d.id,
            cells.len()
        );
        for &(x, y, z) in &cells {
            assert!(
                x < div.0 && y < div.1 && z < div.2,
                "{} marked ({x},{y},{z}) outside its {div:?} lattice",
                d.id
            );
        }
        println!(
            "{:26} {:>2} x {:>2} x {:>2}   {:>4} of {:>4} cells",
            d.id, div.0, div.1, div.2, cells.len(), total
        );
        checked += 1;
    }
    assert!(checked > 10, "only {checked} meshes were reachable to scan");

    // **The slab case, pinned by name** — the one the method was changed for. A wall is solid, so
    // every cell of it is solid; under vertex occupancy this was 4 of 10, top and bottom layers only.
    let wall = layered
        .library
        .get("site/wall")
        .unwrap_or_else(|| panic!("site/wall is in the kit"));
    let div = emerge_core::descriptor::divisions(wall, layered.policy.divisions)
        .unwrap_or_else(|e| panic!("{e}"));
    let glb = emerge_core::glb::Glb::open(Path::new("assets/ozea/wall.glb"))
        .unwrap_or_else(|e| panic!("{e}"));
    let cells = emerge_core::import::occupancy(&glb, div, (0, 0, 0)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        cells.len() as u32,
        emerge_core::descriptor::Subgrid::volume(div),
        "a solid wall is solid in every cell; got {cells:?}"
    );

    // And the discriminating case: a wide doorway is mostly hole, so it must NOT come back full.
    // Without this, "mark everything" would pass the assertion above.
    let wide = layered
        .library
        .get("site/wall_doorway_wide")
        .unwrap_or_else(|| panic!("the wide doorway is in the kit"));
    let wdiv = emerge_core::descriptor::divisions(wide, layered.policy.divisions)
        .unwrap_or_else(|e| panic!("{e}"));
    let wglb = emerge_core::glb::Glb::open(
        Path::new("assets").join(wide.mesh.as_deref().unwrap_or_default()).as_path(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let wcells = emerge_core::import::occupancy(&wglb, wdiv, (0, 0, 0)).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        (wcells.len() as u32) < emerge_core::descriptor::Subgrid::volume(wdiv),
        "a doorway's opening must leave cells open, or this is marking everything: {} of {}",
        wcells.len(),
        emerge_core::descriptor::Subgrid::volume(wdiv)
    );
}
