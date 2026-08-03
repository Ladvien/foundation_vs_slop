//! **Opening a project** — the vocabulary, the library, and the map being edited.
//!
//! A project is a directory with an `assets/` folder in it. `assets/emerge/vocab.ron` says what
//! tokens exist, `assets/emerge/library.ron` says what can be placed, and a descriptor's `mesh` path
//! resolves under `assets/` — which is also Bevy's asset root, so the editor and the game name the
//! same file the same way. Nothing here knows about any particular game.
//!
//! # It refuses to open rather than opening empty
//!
//! Every load failure is fatal and says which file and why. That is the same call
//! `site_editor::source_map` makes — *"a tool that writes level data while unsure which line it is
//! writing is worse than no tool"* — and the failure mode it avoids is the one that wastes an
//! afternoon: an editor that comes up with an empty palette looks exactly like an editor whose
//! project has no assets.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use emerge_core::library::Library;
use emerge_core::map::{Map, MAP_VERSION};
use emerge_core::vocab::{Masks, Vocabularies};

/// Where a project's own files live, under its root. Inside `assets/` rather than beside it, so a
/// descriptor's `mesh` path means the same thing to this editor and to the game that loads the map.
const EMERGE_DIR: &str = "assets/emerge";
const VOCAB: &str = "assets/emerge/vocab.ron";
const LIBRARY: &str = "assets/emerge/library.ron";

/// The opened project.
#[derive(Resource)]
pub struct Project {
    pub root: PathBuf,
    pub vocab: Vocabularies,
    pub library: Library,
    /// Per-descriptor token masks, in library order — resolved once at load so the palette and the
    /// placement rules never re-resolve the same strings.
    pub masks: Vec<Masks>,
    /// The map being edited, and where it will be written.
    pub map: Map,
    pub map_path: PathBuf,
    /// Whether there are edits not yet on disk.
    pub dirty: bool,
}

impl Project {
    /// Read a project, or say exactly what is wrong with it.
    pub fn open(root: &Path, map_name: &str) -> Result<Project, String> {
        let vocab_path = root.join(VOCAB);
        let vocab = Vocabularies::parse(&read(&vocab_path)?)
            .map_err(|e| format!("{}: {e}", vocab_path.display()))?;

        let library_path = root.join(LIBRARY);
        let library = Library::parse(&read(&library_path)?)
            .map_err(|e| format!("{}: {e}", library_path.display()))?;

        // The two-sided pass over the whole set, at open. A prop that rests on a class nothing offers
        // can never be placed, and the moment to say so is now — not when an author wonders why a
        // shelf stays empty.
        let masks = library
            .resolve(&vocab)
            .map_err(|e| format!("{}: {e}", library_path.display()))?;

        let map_path = root.join(EMERGE_DIR).join(map_name);
        // A map that does not exist yet is a new map, not an error: this is how an author starts one.
        // A map that exists and does not parse IS an error — silently replacing it with an empty one
        // would destroy their work on the first save.
        let map = if map_path.is_file() {
            Map::parse(&read(&map_path)?).map_err(|e| format!("{}: {e}", map_path.display()))?
        } else {
            Map {
                version: MAP_VERSION,
                note: Some(format!("Authored in emerge-mapper. Library: {LIBRARY}.")),
                ..Map::default()
            }
        };

        info!(
            "project: {} — {} descriptor(s), {} placement(s), map {}",
            root.display(),
            library.descriptors.len(),
            map.placements.len(),
            map_path.display()
        );

        Ok(Project {
            root: root.to_path_buf(),
            vocab,
            library,
            masks,
            map,
            map_path,
            dirty: false,
        })
    }

    /// Write the map. Atomic, so a crash mid-write cannot leave half a level on disk.
    ///
    /// Serialized by an ordinary serializer with no comment-preserving pass, because an emerge map
    /// keeps its prose in `note:` fields — see `emerge_core::map`. That is the whole reason the
    /// decision was made that way.
    pub fn save(&mut self) -> Result<(), String> {
        self.map.validate()?;
        let text = ron::ser::to_string_pretty(&self.map, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("map: serialize: {e}"))?;
        emerge_core::ron_surgery::save_atomic(&self.map_path, &text)?;
        self.dirty = false;
        Ok(())
    }

    /// The descriptor at a library index, and its resolved masks.
    pub fn entry(&self, ix: usize) -> Option<(&emerge_core::descriptor::Descriptor, Masks)> {
        Some((self.library.descriptors.get(ix)?, *self.masks.get(ix)?))
    }
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}
