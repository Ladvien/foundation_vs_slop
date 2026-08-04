//! **The Site kit resolves to the numbers the game used to compute.**
//!
//! The kit named pieces *and* described them until 2026-08-03: a `glb`, a height, a footprint, a
//! `front`, all inline, plus a `y_scale` computed as `target_height / height`. It now names pieces
//! and the descriptor library describes them, with this facility's architecture layered on from
//! `project.ron`.
//!
//! That move is only safe if the numbers are identical, and "identical" is not something the type
//! system can check across a file boundary. So this pins the **game-visible outcome**: the height a
//! wall is drawn at, the opening an aperture is sized from, the top a mug is seated on. A test that
//! only checked the files parse would have passed while every wall in the hub changed height.
//!
//! The numbers below are the ones the old computation produced, written as literals on purpose. A
//! literal is the only kind of assertion that survives the code it was checking being deleted.

use foundation_vs_slop::site::kit::{
    id_of, load_site_kit, SiteKit, GREYBOX_KIT_PATH, GREYBOX_PROJECT_DIR, SITE_KIT_PATH,
    SITE_PROJECT_DIR,
};
use foundation_vs_slop::site::pieces::{target_height, SitePiece};

fn ozea() -> SiteKit {
    load_site_kit(SITE_KIT_PATH, SITE_PROJECT_DIR).unwrap_or_else(|e| panic!("{e}"))
}

fn greybox() -> SiteKit {
    load_site_kit(GREYBOX_KIT_PATH, GREYBOX_PROJECT_DIR).unwrap_or_else(|e| panic!("{e}"))
}

/// **The architecture still lands where it did.** `y_scale` used to be `target / authored`, computed
/// in `kit.rs`; it is now `stretch_y`, layered on from `project.ron`. Multiplying it back out must
/// return the target the game asked for.
///
/// Checked for **both** kits, because they are the two halves of the swap and their authored heights
/// are wildly different — a rule that only held for the kit which happens to be authored at the
/// target height would hold vacuously.
#[test]
fn every_piece_with_a_target_height_still_reaches_it() {
    for (name, kit) in [("ozea", ozea()), ("greybox", greybox())] {
        for piece in SitePiece::ALL {
            let Some(target) = target_height(*piece) else {
                // No policy height means native size, and native size means no stretch.
                assert_eq!(
                    kit.y_scale(*piece),
                    1.0,
                    "{name}/{piece:?}: nothing asked for a height, so nothing may stretch it"
                );
                continue;
            };
            let drawn = kit.height(*piece) * kit.y_scale(*piece);
            assert!(
                (drawn - target).abs() < 1e-3,
                "{name}/{piece:?}: drawn at {drawn} m but this facility builds it to {target} m"
            );
        }
    }
}

/// **The literals.** These are what the old `target_height / height` division produced, and they are
/// the numbers the hub is built out of. Written out rather than derived, because a derivation is only
/// as good as the thing it derives from — and that thing is what moved.
#[test]
fn the_hubs_own_numbers_are_unchanged() {
    let kit = ozea();

    // Full-height architecture: 2.40 m, and the Ozea meshes are already authored there.
    for piece in [
        SitePiece::Wall,
        SitePiece::WallCorner,
        SitePiece::WallWindow,
        SitePiece::Column,
    ] {
        assert_eq!(kit.y_scale(piece), 1.0, "{piece:?}");
        assert!((kit.height(piece) - 2.40).abs() < 1e-3, "{piece:?}");
    }

    // The counter: a 2 m wall mesh squashed to waist height. The one number in the shipped kit that
    // is unmistakably policy rather than measurement.
    assert!(
        (kit.y_scale(SitePiece::WallLow) - 0.45).abs() < 1e-4,
        "the Records desk is a 2 m wall at 0.45 — got {}",
        kit.y_scale(SitePiece::WallLow)
    );
    assert!((kit.height(SitePiece::WallLow) * kit.y_scale(SitePiece::WallLow) - 0.9).abs() < 1e-3);

    // The doorways, which the ASYNC aperture quad is sized from.
    let (w, h) = kit
        .opening(SitePiece::WallDoorwayWide)
        .unwrap_or_else(|| panic!("the wide doorway has an opening"));
    assert!(w > 0.0 && h > 0.0 && h < kit.height(SitePiece::WallDoorwayWide));
    let (sw, _) = kit
        .opening(SitePiece::WallDoorway)
        .unwrap_or_else(|| panic!("the single doorway has an opening"));
    assert!(w > sw, "the wide doorway opens wider");

    // And the greybox fixture stretches where Ozea does not — the swap, in one number.
    assert!(
        (greybox().y_scale(SitePiece::Wall) - 2.4).abs() < 1e-3,
        "a 1 m greybox module reaches 2.4 m by stretching 2.4x"
    );
}

/// A surface piece seats what rests on it at its *top*, and the top is every transform in order.
/// `top_height` is what a mug's Y comes from, so it is worth pinning rather than trusting.
#[test]
fn a_resting_prop_is_seated_on_the_hosts_actual_top() {
    let kit = ozea();
    for piece in SitePiece::ALL {
        if !kit.is_surface(*piece) {
            continue;
        }
        let expect =
            kit.y_offset(*piece) + kit.height(*piece) * kit.scale(*piece) * kit.y_scale(*piece);
        assert_eq!(kit.top_height(*piece), expect, "{piece:?}");
        assert!(
            kit.top_height(*piece) > 0.0,
            "{piece:?} offers a surface at or below the deck"
        );
    }
}

/// Every id a kit names exists in its project, and the two kits name the **same** ids — which is what
/// makes them alternatives rather than two unrelated sets.
#[test]
fn both_kits_name_the_same_pieces_and_every_id_resolves() {
    let (a, b) = (ozea(), greybox());
    for piece in SitePiece::ALL {
        let want = id_of(*piece);
        assert_eq!(a.id(*piece), want, "the shipped kit's id convention");
        assert_eq!(b.id(*piece), want, "the fixture must name the same pieces");
        // Resolved means it is in the library; `load_site_kit` refuses otherwise.
        assert!(!a.glb(*piece).is_empty(), "{piece:?} has geometry");
        assert!(!b.glb(*piece).is_empty(), "{piece:?} has geometry");
    }
}

/// **An authored map loads.** `assets/emerge/break_room.map.ron` was made in `emerge-mapper` on
/// 2026-08-03 — a fridge, a table, three chairs and a wall light — and it is committed as the first
/// map the editor produced that the game can read.
///
/// It exists as a **fixture**, not as content: the loop it proves (author → save → the game validates
/// the same file with the same rules) had no regression test at all, so a schema change could have
/// broken every authored map with nothing failing.
#[test]
fn the_authored_break_room_still_loads() {
    use emerge_core::map::Map;

    let text = std::fs::read_to_string("assets/emerge/break_room.map.ron")
        .unwrap_or_else(|e| panic!("assets/emerge/break_room.map.ron: {e}"));
    let map = Map::parse(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(map.name, "break_room");
    assert_eq!(map.placements.len(), 6, "a fridge, a table, three chairs, a light");

    // Through the real load path — the library layered with this project's policy, the same call the
    // game and the editor both make — so a descriptor the library stopped defining fails here.
    let library = emerge_core::policy::layered_library(std::path::Path::new("assets/emerge"))
        .unwrap_or_else(|e| panic!("{e}"));
    for p in &map.placements {
        assert!(
            library.get(&p.descriptor).is_some(),
            "{} names `{}`, which the library no longer defines",
            p.id,
            p.descriptor
        );
    }
    // And every height resolves — the check that would catch a stacked piece whose host went away.
    emerge_core::stack::resolve_y(&map, &library).unwrap_or_else(|e| panic!("{e}"));
}
