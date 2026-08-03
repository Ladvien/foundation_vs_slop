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
use emerge_core::library::Library;
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

    match load(&name) {
        Ok(world) => {
            info!(
                "emerge_map: loaded `{}` — {} placement(s) from {} descriptor(s)",
                world.map.name,
                world.map.placements.len(),
                world.library.descriptors.len()
            );
            app.add_plugins(EmergePlugin).insert_resource(world);
        }
        // Loud and specific. The alternative is a world that comes up empty, which reads as "the
        // editor did not save" rather than as "the game could not find the vocabulary".
        Err(e) => error!("emerge_map: cannot load `{name}`: {e}"),
    }
}

/// Read the vocabulary, the library and the map, and check they agree.
fn load(name: &str) -> Result<EmergeWorld, String> {
    let read = |file: &str| -> Result<String, String> {
        let path = std::path::Path::new(EMERGE_DIR).join(file);
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    };

    let vocab = Vocabularies::parse(&read("vocab.ron")?)?;
    let library = Library::parse(&read("library.ron")?)?;
    let map = Map::parse(&read(&naming::map_file_name(name))?)?;
    EmergeWorld::new(library, map, vocab)
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
        let library = Library::parse(
            &std::fs::read_to_string("assets/emerge/library.ron").unwrap_or_else(|e| panic!("{e}")),
        )
        .unwrap_or_else(|e| panic!("{e}"));

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

    /// A map naming a piece the library does not have is refused, not partially loaded.
    #[test]
    fn a_map_with_a_hole_in_it_does_not_load() {
        let vocab = Vocabularies::parse(
            &std::fs::read_to_string("assets/emerge/vocab.ron").unwrap_or_else(|e| panic!("{e}")),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let library = Library::parse(
            &std::fs::read_to_string("assets/emerge/library.ron").unwrap_or_else(|e| panic!("{e}")),
        )
        .unwrap_or_else(|e| panic!("{e}"));

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

    /// The env var takes a NAME and is forced into the same spelling the editor uses, so a map saved
    /// as `galley_deck` is reachable however it is typed on the command line.
    #[test]
    fn the_map_name_is_forced_into_the_editors_spelling() {
        assert_eq!(naming::to_snake_case("Galley Deck"), "galley_deck");
        assert_eq!(naming::map_file_name("galley_deck"), "galley_deck.map.ron");
    }
}
