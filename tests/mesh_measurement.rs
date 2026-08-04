//! **The measurement library against the shipped kit.**
//!
//! `emerge_core::glb` computes the numbers a descriptor records. This checks it against ground truth:
//! `assets/site/kit_ozea.ron`, whose heights and footprints were measured by hand (via
//! `scripts/fbx_to_glb.py`'s `INVENTORY.md`) and have been the game's contract for months.
//!
//! Four fields nothing validated before — `footprint` against the mesh, `scale`, `DoorPiece::opening`
//! and `front` — are listed in `docs/2026-08-03-asset-schema-audit.md` §5. `front` is the sharpest
//! case: its derivation method is *written down* in `site::kit` and was implemented nowhere, having
//! been measured once by hand for two chairs. If the library cannot reproduce those two numbers, the
//! importer that will lean on it is not trustworthy.
//!
//! GPU-free and `App`-free, so it runs in the `cargo test` hard gate.

use std::path::Path;

use foundation_vs_slop::site::kit::{load_site_kit, SITE_KIT_PATH, SITE_PROJECT_DIR};
use foundation_vs_slop::site::pieces::SitePiece;
use emerge_core::glb::{Glb, OriginAlignment};

fn open(rel: &str) -> Glb {
    let path = Path::new("assets").join(rel);
    Glb::open(&path).unwrap_or_else(|e| panic!("{e}"))
}

/// The kit's `height` is what the Site stretches architecture from, and its `footprint` is what every
/// placement rule reserves. Both were measured by hand; the library must agree.
#[test]
fn the_library_reproduces_the_kits_hand_measured_extents() {
    let kit = load_site_kit(SITE_KIT_PATH, SITE_PROJECT_DIR).unwrap_or_else(|e| panic!("site kit: {e}"));

    let mut checked = 0usize;
    let mut skipped = Vec::new();
    for piece in SitePiece::ALL {
        let entry_piece = *piece;
        let path = Path::new("assets").join(&kit.glb(entry_piece));
        if !path.is_file() {
            continue;
        }
        let g = match Glb::open(&path) {
            Ok(g) => g,
            Err(e) => panic!("{e}"),
        };
        // A node scale makes accessor bounds a lie. Report and skip, as
        // `tests/prop_footprint_contract.rs` does — a silently mismeasured pass is worse than none.
        if g.has_node_transform() {
            skipped.push(kit.glb(entry_piece).clone());
            continue;
        }
        let m = g.measure().unwrap_or_else(|e| panic!("{}: {e}", kit.glb(entry_piece)));

        // The kit records SCALED values, so undo the art correction before comparing to the file.
        let scale = kit.scale(*piece);
        let (want_w, want_d) = kit.footprint(entry_piece);
        let (got_w, got_d) = (m.footprint.0 * scale, m.footprint.1 * scale);
        let got_h = m.height * scale;

        // 2 cm: the kit's numbers were read off a generated inventory table and rounded.
        const TOL: f32 = 0.02;
        assert!(
            (got_h - kit.height(entry_piece)).abs() <= TOL,
            "{}: kit says height {:.3} m, the mesh measures {:.3} m",
            kit.glb(entry_piece),
            kit.height(entry_piece),
            got_h
        );
        assert!(
            (got_w - want_w).abs() <= TOL && (got_d - want_d).abs() <= TOL,
            "{}: kit says footprint {:?}, the mesh measures ({:.3}, {:.3})",
            kit.glb(entry_piece),
            kit.footprint(entry_piece),
            got_w,
            got_d
        );
        checked += 1;
    }

    println!("measured {checked} kit meshes; skipped {} for node transforms", skipped.len());
    if !skipped.is_empty() {
        println!("  skipped: {skipped:#?}");
    }
    // A shrinking denominator is how a green test stops meaning anything.
    assert!(
        checked >= 30,
        "expected to measure most of the kit, managed only {checked}"
    );
}

/// **The claim the audit called out.** `site::kit` describes deriving `front` from the XZ centroid of
/// the upper 45% of the mesh, and only `chair` and `command_chair` carry a value — `Some(90.0)`, set
/// by hand. Reproducing exactly those two, and no others, is the test that the method is real.
#[test]
fn the_library_derives_the_two_fronts_the_kit_records_by_hand() {
    let kit = load_site_kit(SITE_KIT_PATH, SITE_PROJECT_DIR).unwrap_or_else(|e| panic!("site kit: {e}"));

    for piece in [SitePiece::Chair, SitePiece::CommandChair] {
        let entry_piece = piece;
        let want = kit
            .front(entry_piece)
            .unwrap_or_else(|| panic!("{piece:?} should carry a hand-measured front"));
        let derived = open(kit.glb(entry_piece))
            .derive_front()
            .unwrap_or_else(|e| panic!("{}: {e}", kit.glb(entry_piece)))
            .unwrap_or_else(|| panic!("{}: derived no front at all", kit.glb(entry_piece)));
        // A quarter turn is the unit the kit authors in; anything inside an eighth of one resolves to
        // the same quarter.
        assert!(
            (derived - want).abs() <= 45.0,
            "{}: kit records front {want}°, the mesh derives {derived:.1}°",
            kit.glb(entry_piece)
        );
    }
}

/// A backless seat has no front, and `None` is a *claim* — different from `Some(0.0)`. The kit says so
/// explicitly: "asserting a facing on a stool would be asserting a fact about the art that is not
/// true."
#[test]
fn a_backless_seat_derives_no_front() {
    let kit = load_site_kit(SITE_KIT_PATH, SITE_PROJECT_DIR).unwrap_or_else(|e| panic!("site kit: {e}"));
    for piece in [SitePiece::Stool, SitePiece::Bench] {
        let entry_piece = piece;
        assert!(kit.front(entry_piece).is_none(), "{piece:?} should carry no front in the kit");
        let derived = open(kit.glb(entry_piece))
            .derive_front()
            .unwrap_or_else(|e| panic!("{}: {e}", kit.glb(entry_piece)));
        assert!(
            derived.is_none(),
            "{}: the kit records no front, but the mesh derives {derived:?}",
            kit.glb(entry_piece)
        );
    }
}

/// `tests/ozea_asset.rs` asserts base-at-origin and XZ-centred to 5 mm by globbing the directory. The
/// library must classify the same assets the same way, in the asset library's own vocabulary.
#[test]
fn the_ozea_kit_classifies_as_base_at_origin_and_centred() {
    let dir = Path::new("assets/ozea");
    let Ok(entries) = std::fs::read_dir(dir) else {
        panic!("assets/ozea must exist");
    };
    let mut checked = 0usize;
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "glb") {
            continue;
        }
        let g = Glb::open(&path).unwrap_or_else(|err| panic!("{err}"));
        if g.has_node_transform() {
            continue;
        }
        assert_eq!(
            g.origin_alignment().unwrap_or_else(|err| panic!("{}: {err}", path.display())),
            OriginAlignment::BaseAtOriginCentred,
            "{} is not base-origined and XZ-centred",
            path.display()
        );
        checked += 1;
    }
    assert!(checked >= 18, "expected the whole Ozea kit, saw {checked}");
}

/// Nothing in the shipped kit is centimetre-authored — but the check exists because one real asset
/// was: `SM_DoorFrame_Double` measured 200.3 units for a 2.003 m door before `fbx_to_glb.py` learned
/// to bake the node scale.
#[test]
fn no_shipped_kit_mesh_looks_centimetre_authored() {
    let kit = load_site_kit(SITE_KIT_PATH, SITE_PROJECT_DIR).unwrap_or_else(|e| panic!("site kit: {e}"));
    for piece in SitePiece::ALL {
        let entry_piece = *piece;
        let path = Path::new("assets").join(&kit.glb(entry_piece));
        if !path.is_file() {
            continue;
        }
        let g = Glb::open(&path).unwrap_or_else(|e| panic!("{e}"));
        if g.has_node_transform() {
            continue;
        }
        let m = g.measure().unwrap_or_else(|e| panic!("{}: {e}", kit.glb(entry_piece)));
        assert!(
            !m.suspect_centimetres,
            "{} measures {:?} — that looks like centimetre data",
            kit.glb(entry_piece),
            m.footprint
        );
    }
}

/// **The property the threshold rests on**, asserted directly so it survives the constant being
/// retuned: a mesh the kit gives a front is far more asymmetric than one it does not.
///
/// This is the robust claim. `FRONT_MIN_OFFSET` is one number picked from five samples; the ordering
/// is a fact about the art. If a future kit narrows this gap, this fails and the threshold — or the
/// whole centroid method — needs revisiting, which is exactly when someone should be told.
#[test]
fn a_seat_with_a_back_is_markedly_more_asymmetric_than_one_without() {
    let kit = load_site_kit(SITE_KIT_PATH, SITE_PROJECT_DIR).unwrap_or_else(|e| panic!("site kit: {e}"));
    let asym = |p: SitePiece| -> f32 {
        open(kit.glb(p))
            .front_detail()
            .unwrap_or_else(|e| panic!("{e}"))
            .0
    };

    let backed = asym(SitePiece::Chair).min(asym(SitePiece::CommandChair));
    let backless = asym(SitePiece::Stool)
        .max(asym(SitePiece::Bench))
        .max(asym(SitePiece::MessTable));

    assert!(
        backed > backless * 3.0,
        "the two populations should separate cleanly: least-asymmetric backed seat {backed:.4} m \
         vs most-asymmetric backless {backless:.4} m"
    );
    assert!(
        backless < emerge_core::glb::FRONT_MIN_OFFSET
            && backed > emerge_core::glb::FRONT_MIN_OFFSET,
        "FRONT_MIN_OFFSET ({}) must sit between them: {backless:.4} .. {backed:.4}",
        emerge_core::glb::FRONT_MIN_OFFSET
    );
}
