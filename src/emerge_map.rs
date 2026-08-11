//! **Loading a map the editor wrote** (`FVS_EMERGE_MAP=<name>`).
//!
//! This is the end of the loop. `emerge-mapper` authors a map, `emerge-core` validates it, and
//! `emerge-bevy` puts it in a world — and until something in `src/` actually called that, the editor
//! was a tool whose output the game could not read. A pipeline nobody has run end to end is a
//! pipeline with an unknown number of breaks in it.
//!
//! ```text
//! FVS_EMERGE_MAP=galley_deck cargo run
//! ```
//!
//! # It refuses loudly
//!
//! Every failure here is fatal to the load and says which file and why: a missing vocabulary, a map
//! that names a descriptor the library does not define, a library whose surface classes do not close.
//! The same call `site_editor::source_map` makes for the same reason — *"a tool that writes level
//! data while unsure which line it is writing is worse than no tool"* — and the failure it avoids is
//! the one that wastes an afternoon: a map that loads with three pieces missing looks exactly like a
//! map somebody authored with three fewer pieces.
//!
//! # Dev-only, and off the determinism path
//!
//! Gated on an environment variable and registered only in `lib::run`, never in `sim_harness`. It
//! spawns cosmetic entities from a file that is not part of any golden, so `snapshot_hash` cannot see
//! it — the same contract `research_room` and `site_editor` hold.

use bevy::prelude::*;
use emerge_bevy::{EmergePlugin, EmergeWorld};
use emerge_core::map::Map;
use emerge_core::naming;
use emerge_core::vocab::Vocabularies;

/// Where the editor keeps a project's files, relative to the asset root.
const EMERGE_DIR: &str = "assets/emerge";

/// Install the loader when `FVS_EMERGE_MAP` names a map.
///
/// Called from `lib::run` beside the other dev tools. Absent the variable this adds nothing at all —
/// not a disabled system, nothing — so a shipped run carries none of it.
pub fn install_if_requested(app: &mut App) {
    let Ok(name) = std::env::var("FVS_EMERGE_MAP") else {
        return;
    };
    // Forced into the one spelling, exactly as the editor does, so `FVS_EMERGE_MAP="Galley Deck"`
    // opens the file the editor wrote rather than failing to find it.
    let name = naming::to_snake_case(&name);
    if name.is_empty() {
        error!("FVS_EMERGE_MAP is set but leaves nothing usable as a map name");
        return;
    }

    // **Where to put it.** `Map::origin` is the map's own answer and is what ships; this override
    // exists because the maps an author is testing are almost always authored at (0,0,0), and the
    // game's camera is not there. `FVS_EMERGE_MAP_AT=x,z` (or `x,y,z`) drops the map somewhere you
    // can see it without editing the file to look at it.
    //
    // It sets the SAME field rather than adding a second notion of where a map is — configuration,
    // not a parallel path.
    let at = std::env::var("FVS_EMERGE_MAP_AT").ok().and_then(|s| parse_at(&s));

    match load(&name).map(|mut world| {
        if let Some(origin) = at {
            info!("emerge_map: FVS_EMERGE_MAP_AT — placing `{name}` at {origin:?}");
            world.map.origin = origin;
        }
        world
    }) {
        Ok(world) => {
            // Counted after expansion, so the number is what the world actually holds rather than
            // what the file happens to list. Counts come from `emerge_core::census`.
            let counted = emerge_core::census::of_map(&world.map);
            let catalog = emerge_core::census::of_catalog(&world.library, &[]);
            info!(
                "emerge_map: loaded `{}` — {} placement(s) from {} descriptor(s)",
                world.map.name, counted.placements, catalog.descriptors
            );
            app.add_plugins(EmergePlugin).insert_resource(world);
        }
        // Loud and specific. The alternative is a world that comes up empty, which reads as "the
        // editor did not save" rather than as "the game could not find the vocabulary".
        Err(e) => error!("emerge_map: cannot load `{name}`: {e}"),
    }
}

/// Parse `x,z` or `x,y,z` into a world origin. Two numbers means the floor stays at zero, which is
/// what someone typing coordinates off a map almost always means.
fn parse_at(s: &str) -> Option<(f32, f32, f32)> {
    // Every rejection says so. The first version returned `None` silently when a component would not
    // parse and only complained about the wrong *count* — so `FVS_EMERGE_MAP_AT=80,11o` looked exactly
    // like not setting the variable at all, which is the failure the paragraph below was written to
    // prevent and did not.
    let mut parts = Vec::new();
    for p in s.split(',') {
        match p.trim().parse::<f32>() {
            Ok(v) => parts.push(v),
            Err(e) => {
                error!("FVS_EMERGE_MAP_AT: `{}` is not a number ({e})", p.trim());
                return None;
            }
        }
    }
    match parts[..] {
        [x, z] => Some((x, 0.0, z)),
        [x, y, z] => Some((x, y, z)),
        // Silence would be worse: a mistyped override that quietly did nothing would look like the
        // map being in the wrong place for some other reason.
        _ => {
            error!("FVS_EMERGE_MAP_AT expects `x,z` or `x,y,z`, got `{s}`");
            None
        }
    }
}

/// Read the vocabulary, the library and the map, and check they agree.
fn load(name: &str) -> Result<EmergeWorld, String> {
    let read = |file: &str| -> Result<String, String> {
        let path = std::path::Path::new(EMERGE_DIR).join(file);
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    };

    let vocab = Vocabularies::parse(&read("vocab.ron")?)?;
    // The same call the editor makes: measurements, then this project's policy over them — and the
    // compositions beside them, validated against the layered library by the same loader. One call,
    // so the editor and the game cannot disagree about what a project contains.
    let layered = emerge_core::policy::layered_library(std::path::Path::new(EMERGE_DIR))?;
    let map = Map::parse(&read(&naming::map_file_name(name))?)?;
    EmergeWorld::with_compositions(
        layered.library,
        map,
        vocab,
        &layered.compositions.compositions,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The project's own files load and agree with each other. This is the loop closing: the editor
    /// writes these, and the game reads them with the same validation.
    ///
    /// No map is required to exist — a map is an author's work, not a shipped asset — so this checks
    /// the halves that DO ship, which is where a break would live anyway.
    #[test]
    fn the_shipped_vocabulary_and_library_load_together() {
        let vocab = Vocabularies::parse(
            &std::fs::read_to_string("assets/emerge/vocab.ron").unwrap_or_else(|e| panic!("{e}")),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        // Through the real load path, so the shipped `project.ron` is exercised rather than skipped.
        let library = emerge_core::policy::layered_library(std::path::Path::new("assets/emerge"))
            .unwrap_or_else(|e| panic!("{e}"))
            .library;

        // An empty map is a valid one — it is what an author starts with.
        let world = EmergeWorld::new(
            library,
            Map {
                name: "empty".into(),
                ..Map::default()
            },
            vocab,
        )
        .unwrap_or_else(|e| panic!("the shipped project does not load: {e}"));

        assert!(
            world.library.descriptors.len() >= 40,
            "expected the converted furniture set, saw {}",
            world.library.descriptors.len()
        );
        // Every descriptor resolved to a mask, which is what makes a tag query one `&`.
        assert_eq!(world.masks.len(), world.library.descriptors.len());
    }

    /// **A stamp becomes rows, and its affordance comes with it.**
    ///
    /// The loop the reference model was chosen for, checked end to end against the *shipped* files: a
    /// map holds one line naming `break_table`, and the world it loads holds the table, both chairs
    /// and the meal location — with the location's props pointing at the rows this stamp produced
    /// rather than at member names nothing would resolve.
    ///
    /// It also pins the thing that would be silently wrong: the rows land at the stamp's position,
    /// turned by its yaw, not at the composition's own local coordinates.
    #[test]
    fn a_stamped_composition_becomes_rows_and_brings_its_affordance() {
        let vocab = Vocabularies::parse(
            &std::fs::read_to_string("assets/emerge/vocab.ron").unwrap_or_else(|e| panic!("{e}")),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let layered = emerge_core::policy::layered_library(std::path::Path::new("assets/emerge"))
            .unwrap_or_else(|e| panic!("{e}"));

        // **The group is carried here, not read off disk.** It used to come from
        // `assets/emerge/compositions.ron`, which made this an asset-contract test — and the contract
        // dissolved the day the project was cleared to author tiles with the editor's own BUILD mode.
        // What it actually pins is the expand-to-rows-and-locations loop, which is a fact about this
        // code rather than about which groups happen to ship, so the fixture belongs with the test.
        // Recovered verbatim from the deleted file, notes trimmed.
        let comps: emerge_core::composition::Compositions = ron::from_str(
            r#"(
                version: 1,
                compositions: [(
                    id: "break_table",
                    envelope: Anchored,
                    members: [
                        (id: "chair_north", body: Descriptor(id: "dining_chair", tip: (0, 0), on: None, patch: None), at: (0.0, -1.0), yaw: 180.0, lift: 0.0),
                        (id: "chair_south", body: Descriptor(id: "dining_chair", tip: (0, 0), on: None, patch: None), at: (0.0, 1.0), yaw: 0.0, lift: 0.0),
                        (id: "table", body: Descriptor(id: "table", tip: (0, 0), on: None, patch: None), at: (0.0, 0.0), yaw: 0.0, lift: 0.0),
                    ],
                    locations: [(
                        id: "meal",
                        props: ["table", "chair_north", "chair_south"],
                        interactions: [(
                            verb: "eat",
                            roles: [(name: "diner", kind: Supporting, min: 1, max: 2, socket_role: Some("diner"), requires: ["eat"])],
                            guard: None,
                            effects: [Restore(drive: "hunger", rate: 0.15)],
                        )],
                    )],
                )],
            )"#,
        )
        .unwrap_or_else(|e| panic!("the fixture group parses: {e}"));

        let map = Map {
            name: "stamped".into(),
            stamps: vec![emerge_core::composition::Stamped {
                id: "mess_a".into(),
                of: "break_table".into(),
                at: (4.0, 0.0),
                yaw: 90.0,
                ..Default::default()
            }],
            ..Map::default()
        };
        let world = EmergeWorld::with_compositions(
            layered.library,
            map,
            vocab,
            &comps.compositions,
        )
        .unwrap_or_else(|e| panic!("the fixture group does not stamp: {e}"));

        let ids: Vec<&str> = world.map.placements.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["mess_a/chair_north", "mess_a/chair_south", "mess_a/table"]);

        // The table sits at the stamp; a chair one metre along the group's +Z lands one metre along
        // +X of it, because a positive yaw turns +X toward -Z.
        let table = world
            .map
            .placements
            .iter()
            .find(|p| p.id == "mess_a/table")
            .unwrap_or_else(|| panic!("no table"));
        assert!((table.at.0 - 4.0).abs() < 1e-4 && table.at.1.abs() < 1e-4, "{:?}", table.at);
        let south = world
            .map
            .placements
            .iter()
            .find(|p| p.id == "mess_a/chair_south")
            .unwrap_or_else(|| panic!("no chair"));
        assert!((south.at.0 - 5.0).abs() < 1e-4 && south.at.1.abs() < 1e-4, "{:?}", south.at);

        // The affordance travelled, repointed, and resolved to real seats — which is the half a
        // geometry-only stamp would have silently dropped.
        assert_eq!(world.map.locations.len(), 1);
        assert_eq!(world.map.locations[0].id, "mess_a/meal");
        assert_eq!(
            world.map.locations[0].props,
            ["mess_a/table", "mess_a/chair_north", "mess_a/chair_south"]
        );
        // **The seats are empty, and that is a fact about the corpus rather than about this path.**
        // `smart::seats_of` builds a seat from a descriptor's `offers.sockets`, and *no descriptor in
        // the shipped library has one* — measured: zero `role:` entries across the whole file. So the
        // location resolves, its roles resolve, and it seats nobody, because nobody has yet authored
        // where a diner stands at a chair.
        //
        // Asserted rather than skipped so that authoring the first socket turns this line red and
        // whoever does it learns immediately that the loop now closes.
        assert!(
            world.seats("mess_a/meal").is_empty(),
            "a descriptor now offers a socket — good. Flip this assertion and the one in \
             `a_stamped_composition_becomes_rows_and_brings_its_affordance`."
        );
    }

    /// A stamp naming a composition nothing defines is refused, not quietly skipped.
    #[test]
    fn a_stamp_of_a_composition_that_does_not_exist_is_refused() {
        let vocab = Vocabularies::parse(
            &std::fs::read_to_string("assets/emerge/vocab.ron").unwrap_or_else(|e| panic!("{e}")),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let layered = emerge_core::policy::layered_library(std::path::Path::new("assets/emerge"))
            .unwrap_or_else(|e| panic!("{e}"));
        let map = Map {
            name: "broken".into(),
            stamps: vec![emerge_core::composition::Stamped {
                id: "ghost".into(),
                of: "no_such_group".into(),
                ..Default::default()
            }],
            ..Map::default()
        };
        let err = EmergeWorld::with_compositions(
            layered.library,
            map,
            vocab,
            &layered.compositions.compositions,
        )
        .err()
        .unwrap_or_else(|| panic!("must refuse"));
        assert!(err.contains("no_such_group"), "{err}");
    }

    /// A map naming a piece the library does not have is refused, not partially loaded.
    #[test]
    fn a_map_with_a_hole_in_it_does_not_load() {
        let vocab = Vocabularies::parse(
            &std::fs::read_to_string("assets/emerge/vocab.ron").unwrap_or_else(|e| panic!("{e}")),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let library = emerge_core::policy::layered_library(std::path::Path::new("assets/emerge"))
            .unwrap_or_else(|e| panic!("{e}"))
            .library;

        let mut map = Map {
            name: "broken".into(),
            ..Map::default()
        };
        map.placements.push(emerge_core::map::Placed {
            id: "a".into(),
            descriptor: "a_piece_that_does_not_exist".into(),
            ..emerge_core::map::Placed::default()
        });
        let err = EmergeWorld::new(library, map, vocab).err().unwrap_or_default();
        assert!(err.contains("does not define"), "{err}");
    }

    #[test]
    fn the_placement_override_takes_two_or_three_numbers() {
        assert_eq!(parse_at("10, -4"), Some((10.0, 0.0, -4.0)));
        assert_eq!(parse_at("10,2,-4"), Some((10.0, 2.0, -4.0)));
        assert_eq!(parse_at(" 1024 , 1024 "), Some((1024.0, 0.0, 1024.0)));
        // A mistyped override must not quietly do nothing.
        assert_eq!(parse_at("nowhere"), None);
        assert_eq!(parse_at("1"), None);
        assert_eq!(parse_at("1,2,3,4"), None);
    }

    /// The env var takes a NAME and is forced into the same spelling the editor uses, so a map saved
    /// as `galley_deck` is reachable however it is typed on the command line.
    #[test]
    fn the_map_name_is_forced_into_the_editors_spelling() {
        assert_eq!(naming::to_snake_case("Galley Deck"), "galley_deck");
        assert_eq!(naming::map_file_name("galley_deck"), "galley_deck.map.ron");
    }
}
