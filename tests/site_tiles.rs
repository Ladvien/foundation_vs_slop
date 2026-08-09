//! **The Site kit's four authored tiles are tiles** — and that claim is the whole of step 4.
//!
//! An **asset-contract** test: its assertions *are* facts about what ships, so it reads the real
//! project on purpose (`crates/emerge-mapper/CLAUDE.md` names this as the deliberate exception to
//! testing against fixtures — checking a fixture here would be checking that the fixture is what the
//! fixture is).
//!
//! # What it is evidence for
//!
//! `docs/2026-08-09-unified-composition.md` proposed **lattice composition**: a bounded composition
//! IS a tile, and the relation between floor, wall and doorway is baked into the tile's local
//! coordinates at authoring time rather than resolved per placement. The plan's step 4 is the step
//! that produces evidence for or against it, and this file is that evidence.
//!
//! The argument it has to carry is precise, and `emerge_core::grammar` already states the other half
//! of it: a tile grammar needs pieces of the grid's size, and *"`site/wall_doorway` is 0.46 x 2.06 m
//! and would overlap its neighbour by about a metre, while `site/wall_corner` at 0.22 x 0.22 m would
//! leave three quarters of a metre of gap — every declared adjacency satisfied, and geometry nobody
//! can use."* **Not one wall piece in the Site kit is the size of the cell.** A group of
//! floor-plus-wall is, exactly, and that is what these tiles demonstrate.
//!
//! # The numbers are literals on purpose
//!
//! `-0.45` is not a rounding of anything: it is half the tile minus half the wall's own 0.1 m
//! thickness, and it is **not a multiple of `grid::SNAP`**, which is why the Compose tab needed a
//! flush verb rather than only a lattice step. If a future edit makes these round numbers, the
//! flush verb has stopped doing its job and this test should go red rather than be re-pinned.

use std::path::Path;

use emerge_core::composition::{self, Composition, Compositions, Envelope};
use emerge_core::library::Library;

const SITE: &str = "assets/emerge/site";

/// The tile every one of these groups claims, metres. `site/floor` is 1.0 x 1.0 and the walls stand
/// 2.4 m, so this is measured rather than chosen.
const TILE: (f32, f32, f32) = (1.0, 2.4, 1.0);

/// The grid step the map is read on — `emerge-mapper`'s `CELL`, and what `grammar::learn` is given.
const CELL: f32 = 1.0;

fn load() -> (Library, Vec<Composition>) {
    let dir = Path::new(SITE);
    let lib_text = std::fs::read_to_string(dir.join("library.ron"))
        .unwrap_or_else(|e| panic!("the site library must be readable: {e}"));
    let library =
        Library::parse(&lib_text).unwrap_or_else(|e| panic!("the site library must parse: {e}"));
    let comp_text = std::fs::read_to_string(dir.join(Compositions::FILE))
        .unwrap_or_else(|e| panic!("the site tiles must be readable: {e}"));
    let set = Compositions::parse(&comp_text)
        .unwrap_or_else(|e| panic!("the site tiles must parse: {e}"));
    (library, set.compositions)
}

fn tile<'a>(comps: &'a [Composition], id: &str) -> &'a Composition {
    comps
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("`{id}` must be one of the site tiles"))
}

/// What a face presents, flattened to the distinct tokens on it in order.
fn face(iface: &composition::Interface, dir: usize) -> Vec<Option<String>> {
    iface.faces[dir].iter().map(|b| b.token.clone()).collect()
}

/// **They are a valid set**, by the same call the editor's commit door makes before writing.
#[test]
fn the_site_tiles_validate() {
    let (library, comps) = load();
    composition::validate(&comps, &library)
        .unwrap_or_else(|e| panic!("the shipped site tiles must be a valid set: {e}"));
    assert_eq!(comps.len(), 4, "four tiles were authored; found {}", comps.len());
}

/// **Every tile claims exactly the grammar's cell** — which is the point of the exercise.
///
/// `grammar::learn` refuses a piece whose footprint is not the cell size, within `CELL_EPSILON`
/// (1e-4). Not one wall piece in the kit passes that. Every group here does.
#[test]
fn every_tile_is_exactly_one_cell_and_no_wall_piece_is() {
    let (library, comps) = load();
    for c in &comps {
        let Envelope::Bounded { size } = c.envelope else {
            panic!("`{}` is anchored; a tile has to claim a tile", c.id);
        };
        assert_eq!(size, TILE, "`{}` claims {size:?}, not the cell", c.id);
        assert!(
            (size.0 - CELL).abs() <= emerge_core::grammar::CELL_EPSILON
                && (size.2 - CELL).abs() <= emerge_core::grammar::CELL_EPSILON,
            "`{}` would be refused by grammar::learn as the wrong size for a {CELL} m cell",
            c.id
        );
    }

    // The other half of the argument, measured rather than asserted from memory: the raw pieces
    // these tiles are made of could not be tiles themselves.
    for id in ["site/wall", "site/wall_corner", "site/wall_doorway", "site/wall_header"] {
        let d = library
            .get(id)
            .unwrap_or_else(|| panic!("`{id}` must be in the site library"));
        let (w, dep) = emerge_core::descriptor::placed_footprint(d)
            .unwrap_or_else(|| panic!("`{id}` must be measured"));
        assert!(
            (w - CELL).abs() > emerge_core::grammar::CELL_EPSILON
                || (dep - CELL).abs() > emerge_core::grammar::CELL_EPSILON,
            "`{id}` is {w} x {dep} m — if a wall piece is now cell-sized, these tiles may be \
             unnecessary and the argument for composition-as-tile is weaker than it was"
        );
    }
}

/// **Open floor presents nothing, on all four sides.**
///
/// And nothing is a token in its own right rather than a wildcard — `adjacency`'s rule — so this is
/// the tile every wall tile is checked against, not a permissive default.
#[test]
fn the_floor_tile_presents_nothing_anywhere() {
    let (library, comps) = load();
    let c = tile(&comps, "site/tile_floor");
    let iface = composition::interface(c, &comps, &library, 1)
        .unwrap_or_else(|e| panic!("{e}"))
        .expect("a bounded tile has an interface");
    assert!(iface.is_clean(), "{:?}", iface.faults);
    for dir in [emerge_core::wfc::N, emerge_core::wfc::E, emerge_core::wfc::S, emerge_core::wfc::W] {
        assert_eq!(
            face(&iface, dir),
            vec![None],
            "open floor must present nothing on every face; dir {dir} read {:?}",
            iface.faces[dir]
        );
    }
}

/// **A wall tile presents `wall` on the side it has a wall on, and nothing on the other three.**
///
/// This is the assertion the whole design rests on: the group's boundary is read off its members,
/// so a tile made of a floor and a wall says "wall" to whatever abuts its north face without anyone
/// authoring an interface anywhere.
#[test]
fn the_wall_tile_presents_wall_to_the_north_only() {
    let (library, comps) = load();
    let c = tile(&comps, "site/tile_wall_n");
    let iface = composition::interface(c, &comps, &library, 1)
        .unwrap_or_else(|e| panic!("{e}"))
        .expect("bounded");
    assert!(iface.is_clean(), "{:?}", iface.faults);
    assert_eq!(
        face(&iface, emerge_core::wfc::N),
        vec![Some("wall".to_owned())],
        "north read {:?}",
        iface.faces[emerge_core::wfc::N]
    );
    for (dir, name) in [
        (emerge_core::wfc::E, "east"),
        (emerge_core::wfc::S, "south"),
        (emerge_core::wfc::W, "west"),
    ] {
        assert_eq!(
            face(&iface, dir),
            vec![None],
            "{name} must present nothing, read {:?}",
            iface.faces[dir]
        );
    }
}

/// A corner is two seatings of one piece, and presents `wall` on both of its sides.
///
/// The cross product of wall positions is not authored as separate meshes — Karth & Smith's
/// multi-tile modules via edge constraints (`10.1145/3337722.3341845`), and the reason nesting and
/// composition were kept as different jobs in the design.
#[test]
fn the_corner_tile_presents_wall_on_two_sides() {
    let (library, comps) = load();
    let c = tile(&comps, "site/tile_corner_nw");
    let iface = composition::interface(c, &comps, &library, 1)
        .unwrap_or_else(|e| panic!("{e}"))
        .expect("bounded");
    assert!(iface.is_clean(), "{:?}", iface.faults);
    for (dir, name) in [(emerge_core::wfc::N, "north"), (emerge_core::wfc::W, "west")] {
        assert_eq!(
            face(&iface, dir),
            vec![Some("wall".to_owned())],
            "{name} read {:?}",
            iface.faces[dir]
        );
    }
    for (dir, name) in [(emerge_core::wfc::E, "east"), (emerge_core::wfc::S, "south")] {
        assert_eq!(face(&iface, dir), vec![None], "{name} read {:?}", iface.faces[dir]);
    }
}

/// **The doorway is the first shipped thing with a vertically banded face.**
///
/// Step 2 measured that all 192 faces in both kits are uniform in y, and kept vertical variation
/// representable anyway on the argument that it was *"a property of the descriptors and not of the
/// format"* — that `interface` skips a member whose height misses the sample, so a group mixing a
/// low piece with a tall one bands vertically the moment one is authored. This is that group: a
/// lintel across the top and an opening beneath it.
///
/// It also records why `site/wall_doorway` is not used. At 0.46 x 2.06 m it fits neither a 1 m cell
/// nor a 2 m one, so a solver would lay it at a spacing unrelated to its extent.
#[test]
fn the_doorway_tile_is_a_lintel_over_an_opening() {
    let (library, comps) = load();
    let c = tile(&comps, "site/tile_doorway_n");
    let iface = composition::interface(c, &comps, &library, 1)
        .unwrap_or_else(|e| panic!("{e}"))
        .expect("bounded");
    assert!(iface.is_clean(), "{:?}", iface.faults);

    let north = &iface.faces[emerge_core::wfc::N];
    assert!(
        north.len() >= 2,
        "the doorway's north face must band vertically — read {north:?}"
    );
    let bottom = north.first().expect("at least one band");
    let top = north.last().expect("at least one band");
    assert_eq!(bottom.token, None, "the opening is at the bottom: {bottom:?}");
    assert_eq!(
        top.token.as_deref(),
        Some("wall"),
        "the lintel is at the top: {top:?}"
    );
    assert!(
        top.y.0 > bottom.y.1 - 1e-6 && top.y.1 > top.y.0,
        "the bands must stack, not overlap: {bottom:?} then {top:?}"
    );

    // And the doorway piece the kit ships is the wrong size to be a tile, which is why it is absent.
    let d = library.get("site/wall_doorway").expect("in the library");
    let (w, dep) = emerge_core::descriptor::placed_footprint(d).expect("measured");
    assert!(
        (dep - CELL).abs() > emerge_core::grammar::CELL_EPSILON,
        "site/wall_doorway is {w} x {dep} m; if it became cell-sized it could be used directly"
    );
}

/// **The wall sits where no lattice step could put it**, and that is load-bearing.
///
/// Half the tile minus half the wall's own thickness. If this ever becomes a multiple of
/// `grid::SNAP` the flush verb has stopped being necessary — go and check why rather than re-pinning
/// the number.
#[test]
fn the_walls_are_flush_and_off_the_seating_lattice() {
    let (library, comps) = load();
    let wall = library.get("site/wall").expect("in the library");
    let (thickness, _) = emerge_core::descriptor::placed_footprint(wall).expect("measured");
    let want = TILE.0 * 0.5 - thickness * 0.5;
    assert!((want - 0.45).abs() < 1e-6, "the flush offset is {want}, not 0.45");
    assert!(
        (want / emerge_core::grid::SNAP).fract().abs() > 1e-6,
        "{want} is on the {} m lattice, so the flush verb would be unnecessary",
        emerge_core::grid::SNAP
    );

    for (id, member, at) in [
        ("site/tile_wall_n", "wall_north", (0.0, -want)),
        ("site/tile_corner_nw", "wall_north", (0.0, -want)),
        ("site/tile_corner_nw", "wall_west", (-want, 0.0)),
    ] {
        let m = tile(&comps, id)
            .members
            .iter()
            .find(|m| m.id == member)
            .unwrap_or_else(|| panic!("`{id}` must have a member `{member}`"));
        assert!(
            (m.at.0 - at.0).abs() < 1e-6 && (m.at.1 - at.1).abs() < 1e-6,
            "`{id}`/`{member}` is at {:?}, not flush at {at:?}",
            m.at
        );
    }
}

/// **A room built from these tiles meets itself without a fault.**
///
/// The tests above check each tile alone. This is the half they structurally cannot reach: four
/// correct tiles can still disagree the moment they abut, which is the whole reason
/// [`emerge_core::adjacency::faults`] exists and the reason a tile grammar is worth anything.
///
/// A 3 x 3 room, walls along the north and west runs and a corner where they meet — stamped, expanded
/// through exactly the call the game makes, stacked, and then checked seam by seam:
///
/// ```text
///        x=-1      x=0       x=+1
///  z=-1  corner    wall_n    wall_n
///  z= 0  wall_w    floor     floor
///  z=+1  wall_w    floor     floor
/// ```
///
/// The west run is **the same `tile_wall_n` stamped at yaw 90**, not a fifth authored tile. That is
/// the claim about the cross product being composed rather than enumerated, made concrete: one
/// authored wall tile serves all four orientations.
#[test]
fn a_room_of_tiles_meets_without_a_fault() {
    let (library, comps) = load();

    let stamp = |id: &str, at: (f32, f32), yaw: f32, n: usize| composition::Stamped {
        id: format!("s{n}"),
        of: id.to_owned(),
        at,
        yaw,
        ..Default::default()
    };
    let mut stamps = Vec::new();
    let mut n = 0;
    for (x, z) in [(-1.0f32, -1.0f32), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (-1.0, 1.0)] {
        let (id, yaw) = match (x, z) {
            (-1.0, -1.0) => ("site/tile_corner_nw", 0.0),
            // The north run.
            (_, -1.0) => ("site/tile_wall_n", 0.0),
            // The west run — the SAME tile, turned. Bevy's yaw takes +X toward -Z, so a quarter
            // carries the north wall onto the west face.
            _ => ("site/tile_wall_n", 90.0),
        };
        stamps.push(stamp(id, (x, z), yaw, n));
        n += 1;
    }
    for (x, z) in [(0.0f32, 0.0f32), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        stamps.push(stamp("site/tile_floor", (x, z), 0.0, n));
        n += 1;
    }

    let map = emerge_core::map::Map {
        version: emerge_core::map::MAP_VERSION,
        name: "tile_patch".to_owned(),
        origin: (0.0, 0.0, 0.0),
        bounds: (3.0, TILE.1, 3.0),
        placements: Vec::new(),
        stamps: stamps.clone(),
        locations: Vec::new(),
        note: None,
    };

    // Exactly the call the game and the editor both make.
    let expanded = composition::expand(&map, &stamps, &comps, &library)
        .unwrap_or_else(|e| panic!("a room of shipped tiles must expand: {e}"));
    assert_eq!(
        expanded.placements.len(),
        // 4 floor-only + 4 with one wall + 1 corner with two = 9 floors + 6 walls.
        15,
        "expanded to {} rows: {:?}",
        expanded.placements.len(),
        expanded.placements.iter().map(|p| &p.id).collect::<Vec<_>>()
    );

    let mut full = map.clone();
    full.placements.extend(expanded.placements.iter().cloned());
    emerge_core::stack::resolve_y(&full, &library)
        .unwrap_or_else(|e| panic!("every row in the room must have a height: {e}"));

    // **The seam check.** Two pieces that touch must agree about what they present where they touch.
    let faults = emerge_core::adjacency::faults(&full, &library, 1);
    assert!(
        faults.is_empty(),
        "a room of these tiles disagrees with itself at {} seam(s):\n  {}",
        faults.len(),
        faults.iter().map(|f| f.message.clone()).collect::<Vec<_>>().join("\n  ")
    );
}

/// **One authored wall tile serves all four orientations**, which is the cross-product claim.
///
/// Stamped at each quarter, the north wall lands on north, west, south and east in turn. If this
/// ever needed four authored tiles instead, the argument that composition beats enumeration would be
/// materially weaker and step 5's scope would grow.
#[test]
fn one_wall_tile_covers_four_orientations() {
    let (library, comps) = load();
    let want = [
        (0.0f32, (0.0f32, -0.45f32)),
        (90.0, (-0.45, 0.0)),
        (180.0, (0.0, 0.45)),
        (270.0, (0.45, 0.0)),
    ];
    for (yaw, at) in want {
        let map = emerge_core::map::Map {
            version: emerge_core::map::MAP_VERSION,
            name: "one".to_owned(),
            origin: (0.0, 0.0, 0.0),
            bounds: (TILE.0, TILE.1, TILE.2),
            placements: Vec::new(),
            stamps: Vec::new(),
            locations: Vec::new(),
            note: None,
        };
        let stamps = vec![composition::Stamped {
            id: "s".to_owned(),
            of: "site/tile_wall_n".to_owned(),
            at: (0.0, 0.0),
            yaw,
            ..Default::default()
        }];
        let expanded = composition::expand(&map, &stamps, &comps, &library)
            .unwrap_or_else(|e| panic!("yaw {yaw}: {e}"));
        let wall = expanded
            .placements
            .iter()
            .find(|p| p.descriptor == "site/wall")
            .unwrap_or_else(|| panic!("yaw {yaw} produced no wall"));
        assert!(
            (wall.at.0 - at.0).abs() < 1e-5 && (wall.at.1 - at.1).abs() < 1e-5,
            "at yaw {yaw} the wall landed at {:?}, wanted {at:?}",
            wall.at
        );
    }
}
