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

/// **One shipped mesh cannot be measured, and it is the right call.**
///
/// `low_poly_flashlight.glb` carries non-uniform node scales on two small parts (`switch_base` at
/// 0.16/0.28/0.06 and `switch_button`), so its accessor bounds are not its world size and anything
/// measured from them would be confidently wrong. `tests/prop_footprint_contract.rs` skips this class
/// for the same reason — *"a silently mismeasured pass would be worse than no pass"*.
///
/// Nothing is broken by it today: `squad.rs` places the flashlight at a hardcoded scale and
/// orientation rather than from a measured footprint. It would matter the moment someone wanted it in
/// a library, and the fix is a re-export with the transform baked into the vertices.
///
/// So this pins the known one. A SECOND unmeasurable mesh fails, which is the point.
#[test]
fn only_the_known_unmeasurable_mesh_is_unmeasurable() {
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
    assert_eq!(
        blocked.len(),
        1,
        "expected only the known flashlight, got {}:\n  {}",
        blocked.len(),
        blocked.join("\n  ")
    );
    assert!(
        blocked[0].contains("low_poly_flashlight"),
        "the unmeasurable mesh changed: {}",
        blocked[0]
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
    for dir in ["low_poly_furniture", "kenney_prototype-kit"] {
        let path = root.join(dir);
        if !path.is_dir() {
            continue;
        }
        for c in import::scan(root, &path, &lib).unwrap_or_else(|e| panic!("{e}")) {
            assert!(
                !known.contains(&c.mesh.as_str()),
                "{} is already in the library and was offered anyway",
                c.mesh
            );
        }
    }
}

/// Print the report for one kit. Not an assertion — the output being readable is most of the feature,
/// and `cargo test -- --nocapture` is where anyone judging that will look.
#[test]
fn the_report_reads_like_something_a_person_would_use() {
    let root = Path::new("assets");
    let candidates = import::scan(root, &root.join("ozea"), &library())
        .unwrap_or_else(|e| panic!("{e}"));
    for c in candidates.iter().take(6) {
        println!("\n{}  ->  id `{}`", c.mesh, c.proposed.id);
        if let Some(m) = c.measured {
            println!(
                "  {:.2} x {:.2} m footprint, {:.2} m tall, {} tris",
                m.footprint.0, m.footprint.1, m.height, c.triangles
            );
        }
        for f in &c.findings {
            println!("  [{:?}] {}", f.severity, f.message);
            if let Some(fix) = &f.fix {
                println!("         -> {fix}");
            }
        }
    }
}
