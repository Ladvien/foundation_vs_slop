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

use std::path::Path;

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
    // The armchair arrived with the lattice (`5ba484a`) as the fixture for the off-square-yaw rule —
    // `adjacency.rs` names it in `an_undeclared_tile_at_an_odd_yaw_is_not_a_fault` — and this count
    // was not moved with it, so the fixture test has been failing since.
    assert_eq!(
        map.placements.len(),
        7,
        "a fridge, a table, three chairs, a light, and the yaw-240 armchair"
    );

    // Through the real load path — the library layered with this project's policy, the same call the
    // game and the editor both make — so a descriptor the library stopped defining fails here.
    let library = emerge_core::policy::layered_library(std::path::Path::new("assets/emerge"))
        .unwrap_or_else(|e| panic!("{e}"))
        .library;
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

/// **What the shipped kits' lattices actually come out as**, now that divisions are derived from a
/// piece's own size and the project's `divisions` rather than authored per descriptor.
///
/// Worth pinning against the real kit rather than only against synthetic extents, because the real
/// kit turned out to say something the synthetic case could not: see below.
#[test]
fn the_site_kit_derives_the_lattices_its_architecture_implies() {
    let layered = emerge_core::policy::layered_library(std::path::Path::new("assets/emerge/site"))
        .unwrap_or_else(|e| panic!("{e}"));
    let div = |id: &str| {
        let d = layered
            .library
            .get(id)
            .unwrap_or_else(|| panic!("{id} is in the kit"));
        emerge_core::descriptor::divisions(d, layered.policy.divisions)
            .unwrap_or_else(|e| panic!("{e}"))
    };

    // The 2.40 m pieces all agree, which is the property the change exists to give: a wall, a
    // corner, a window and a column present the same five rows and their edge tokens are comparable.
    let wall = div("site/wall");
    assert_eq!(wall.1, 5, "a 2.40 m wall is five 0.5 m layers");
    for id in ["site/wall_corner", "site/wall_window", "site/column"] {
        assert_eq!(div(id).1, wall.1, "{id} must present the same face height as a wall");
    }

    // **And the doorway deliberately does not.** This facility builds WallDoorway to 2.00 m and
    // WallHeader to 0.40 m — `project.ron` says so in as many words — because a doorway is a 2 m
    // opening with a header stacked above it to reach the wall's 2.40 m. So a doorway's face is four
    // rows against a wall's five, and `adjacency::may_abut` refuses them.
    //
    // That refusal is the honest answer and it is new. Under the old per-descriptor `div: (3,3,3)`
    // both pieces had three layers whatever their height, so they "matched" — a doorway's 0.67 m
    // band compared against a wall's 0.80 m band, which is precisely the incommensurability this
    // change removes. Whether the vertical seam should instead be checked against the
    // doorway-plus-header *stack* is a real question, and it is now answerable because the numbers
    // are real; it was not before.
    assert_eq!(div("site/wall_doorway").1, 4, "a 2.01 m doorway is four layers");
    assert_eq!(div("site/wall_header").1, 1, "a 0.40 m header is one");
    assert_ne!(
        div("site/wall_doorway").1,
        wall.1,
        "a doorway is shorter than a wall, and the lattice must say so rather than pretend otherwise"
    );
}

/// The default (furniture) project's lattices, and the guard that nothing derives an absurd one.
#[test]
fn the_furniture_library_derives_workable_lattices() {
    let layered = emerge_core::policy::layered_library(std::path::Path::new("assets/emerge"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(layered.policy.divisions, 1, "the shipped setting is a 0.5 m subunit");

    for d in &layered.library.descriptors {
        let div = emerge_core::descriptor::divisions(d, layered.policy.divisions)
            .unwrap_or_else(|e| panic!("{e}"));
        let volume = emerge_core::descriptor::Subgrid::volume(div);
        assert!(volume > 0, "{} derives an empty lattice", d.id);
        assert!(
            volume <= emerge_core::library::MAX_LATTICE_CELLS,
            "{} derives {volume} cells, past the ceiling",
            d.id
        );
    }
}

/// **The first authored edge tokens in this repo, and what they prove.**
///
/// `site/wall`, `wall_corner`, `wall_window` and `column` carry `wall` on their run-faces — the N and
/// S faces, since these pieces are thin on X and run along Z. All four are 2.40 m, so all four derive
/// five layers, so all four present a five-cell face. That agreement is the whole point of deriving
/// divisions from a piece's own size: under the old per-descriptor `div: (3,3,3)` a 2.40 m wall and a
/// 0.22 m corner both had three layers, and comparing them compared bands of different heights.
#[test]
fn the_authored_run_faces_let_the_full_height_family_meet() {
    let layered = emerge_core::policy::layered_library(Path::new("assets/emerge/site"))
        .unwrap_or_else(|e| panic!("{e}"));
    let div = layered.policy.divisions;

    // Every piece that carries tokens presents the same face size, or they cannot agree at all.
    let face_len = |id: &str| {
        let d = layered.library.get(id).unwrap_or_else(|| panic!("{id}"));
        let (dx, dy, _) = emerge_core::descriptor::divisions(d, div)
            .unwrap_or_else(|e| panic!("{e}"));
        dx * dy
    };
    for id in ["site/wall", "site/wall_corner", "site/wall_window", "site/column"] {
        assert_eq!(face_len(id), 5, "{id} must present five cells on its run-face");
        let g = layered
            .library
            .get(id)
            .and_then(|d| d.subgrid.as_ref())
            .unwrap_or_else(|| panic!("{id} carries no lattice — the tokens were lost"));
        assert!(
            g.cells.iter().any(|c| c.edge.as_deref() == Some("wall")),
            "{id} carries a lattice with no `wall` token in it"
        );
    }

    // A run of them, end to end along Z, reports nothing. `wall` is 1.00 m long, so its centres sit
    // half a metre apart; the window is 2.00 and the column 0.80.
    let at = |id: &str, d: &str, z: f32| emerge_core::map::Placed {
        id: id.into(),
        descriptor: d.into(),
        at: (0.25, z),
        yaw: 0.0,
        ..emerge_core::map::Placed::default()
    };
    let run = emerge_core::map::Map {
        name: "run".into(),
        placements: vec![
            at("w1", "site/wall", 0.5),
            at("w2", "site/wall", 1.5),
            at("win", "site/wall_window", 3.0),
        ],
        ..emerge_core::map::Map::default()
    };
    let faults =
        emerge_core::adjacency::faults(&run, &layered.library, emerge_core::grid::SNAP, div);
    assert!(
        faults.is_empty(),
        "a run of full-height pieces must agree:\n{}",
        faults.iter().map(|f| f.message.clone()).collect::<Vec<_>>().join("\n")
    );
}

/// **The open question, pinned as behaviour.**
///
/// A doorway is 2.01 m against the wall's 2.40, so it derives four layers to the wall's five. Give it
/// the same token and the seam is refused — not because the tokens disagree, but because the faces
/// are different lengths. The kit reaches 2.40 m by stacking a 0.40 m header above the doorway, so
/// the wall's fifth row is the header's only row.
///
/// `may_abut` compares whole faces, so it cannot see that. Whether it should instead compare the part
/// that overlaps — which would fix this and the horizontal version of it, a 3 m wall meeting a 1 m
/// doorway — is the decision this test exists to keep visible. **It asserts the current behaviour,
/// not the desired one**, so whoever changes the rule has to come here and say so.
#[test]
fn a_doorway_is_one_row_short_of_a_wall_and_the_fault_says_so() {
    let layered = emerge_core::policy::layered_library(Path::new("assets/emerge/site"))
        .unwrap_or_else(|e| panic!("{e}"));
    let div = layered.policy.divisions;
    let mut lib = layered.library.clone();

    // Author the doorway the obvious way: the same token, on its own four-layer run-face.
    let door = lib
        .descriptors
        .iter_mut()
        .find(|d| d.id == "site/wall_doorway")
        .unwrap_or_else(|| panic!("the doorway is in the kit"));
    door.subgrid = Some(emerge_core::descriptor::Subgrid {
        cells: (0..4)
            .flat_map(|y| [(0, y, 0), (0, y, 3)])
            .map(|at| emerge_core::descriptor::SubCell {
                at,
                edge: Some("wall".into()),
                ..emerge_core::descriptor::SubCell::default()
            })
            .collect(),
    });

    let at = |id: &str, d: &str, z: f32| emerge_core::map::Placed {
        id: id.into(),
        descriptor: d.into(),
        at: (0.25, z),
        yaw: 0.0,
        ..emerge_core::map::Placed::default()
    };
    let map = emerge_core::map::Map {
        name: "door".into(),
        placements: vec![at("w1", "site/wall", 0.5), at("d1", "site/wall_doorway", 2.03)],
        ..emerge_core::map::Map::default()
    };
    let faults = emerge_core::adjacency::faults(&map, &lib, emerge_core::grid::SNAP, div);
    assert_eq!(faults.len(), 1, "{faults:#?}");
    let m = &faults[0].message;
    // Four against five, with the same token in every cell — which is what makes it a question about
    // the rule rather than about the tokens.
    assert!(m.contains("[wall wall wall wall]"), "{m}");
    assert!(m.contains("[wall wall wall wall wall]"), "{m}");
}

/// **Two kits that build the same facility derive the same vertical lattice.**
///
/// This is only true because `descriptor::divisions` reads `align.stretch_y`. Before it did, the
/// Site kit's 2.40 m wall derived five layers and `site_greybox`'s 1 m module — stretched 2.4x to
/// stand at the same 2.40 m — derived **two**: a lattice describing a piece a third of the height of
/// the one in the world, and a token authored against one meaning nothing against the other.
///
/// Asserted only for pieces this facility states a height for. A piece with no target stretches by
/// 1.0 in both kits, so any difference between them is a genuine difference in the meshes rather
/// than something the policy layer was meant to reconcile.
///
/// The horizontal divisions are *expected* to differ: footprints are not stretched, and a greybox
/// module really is a different shape from its Ozea counterpart. Only the axis the policy speaks
/// about has to agree.
#[test]
fn both_kits_derive_the_same_layers_for_every_piece_with_a_stated_height() {
    let site = emerge_core::policy::layered_library(Path::new("assets/emerge/site"))
        .unwrap_or_else(|e| panic!("{e}"));
    let grey = emerge_core::policy::layered_library(Path::new("assets/emerge/site_greybox"))
        .unwrap_or_else(|e| panic!("{e}"));

    let layers = |l: &emerge_core::policy::Layered, id: &str| {
        let d = l.library.get(id).unwrap_or_else(|| panic!("{id}"));
        emerge_core::descriptor::divisions(d, l.policy.divisions)
            .unwrap_or_else(|e| panic!("{e}"))
            .1
    };

    let mut checked = 0usize;
    for piece in SitePiece::ALL {
        if target_height(*piece).is_none() {
            continue;
        }
        let id = id_of(*piece).to_string();
        assert_eq!(
            layers(&site, &id),
            layers(&grey, &id),
            "{id}: the two kits build it to the same height, so its lattice must have the same \
             number of layers — site {} vs greybox {}",
            layers(&site, &id),
            layers(&grey, &id)
        );
        checked += 1;
    }
    assert!(checked >= 6, "only {checked} pieces state a height; the check is near-vacuous");

    // And the number is the one the facility asked for: 2.40 m of wall on a 0.5 m subunit is five.
    assert_eq!(layers(&site, "site/wall"), 5);
    assert_eq!(layers(&grey, "site/wall"), 5);
}
