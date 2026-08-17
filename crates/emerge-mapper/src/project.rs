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
use emerge_core::naming;
use emerge_core::vocab::{Masks, Vocabularies};

/// Where a project's own files live, under its root. Inside `assets/` rather than beside it, so a
/// descriptor's `mesh` path means the same thing to this editor and to the game that loads the map.
const EMERGE_DIR: &str = "assets/emerge";
const VOCAB: &str = "assets/emerge/vocab.ron";
const LIBRARY_FILE: &str = "library.ron";

/// The opened project.
#[derive(Resource)]
pub struct Project {
    pub root: PathBuf,
    /// **The kit's directory** — `assets/emerge`, or a named subdirectory of it.
    ///
    /// A *kit* is a library and its policy layer: `assets/emerge/site/` holds the 45 architectural
    /// pieces (wall, corner, doorway, header, column, pipe) with their own patches sizing walls to a
    /// 2.40 m facility, and the default directory holds furniture. `policy::layered_library` already
    /// reads exactly one directory, so a kit is a path and nothing else.
    ///
    /// **The vocabulary is not per-kit** and stays at the root: tokens are what this *project* means,
    /// and a kit that could redefine them would be a second vocabulary to keep in step.
    ///
    /// Resolved once, here, so nothing downstream carries an `Option` or rebuilds this path. Maps are
    /// written beside their kit, because a map means nothing without the library that draws it.
    pub emerge_dir: PathBuf,
    /// Where [`Self::measured`] came from and where it is written back.
    pub library_path: PathBuf,
    pub vocab: Vocabularies,
    /// **`library.ron` as it sits on disk** — the measurements, with no policy layered on.
    ///
    /// **Every edit lands here and this is what gets written.** `write_library` used to serialize
    /// [`Self::library`] back over `library_path`, which meant toggling one lattice cell under
    /// `--kit site` wrote SCP-9191's stretched 2.40 m wall heights into the measurements file the kit
    /// exists to share — and the next load applied the patches again on top of them. A library is
    /// measurements; the architecture is `project.ron`'s, and it must not leak downward.
    pub measured: Library,
    /// **Every composition this project can stamp**, validated against [`Self::library`] at open.
    ///
    /// Read from `compositions.ron` beside the library, or empty when the project has none — which is
    /// the same state as a file with none in it, not a degraded one.
    pub compositions: emerge_core::composition::Compositions,
    /// The measurements with [`Self::policy`] applied — what the game would place, and what every
    /// reader here (the palette, the preview, the flood fill, the fault check) uses.
    ///
    /// Derived, never written. Rebuilt from `measured` after every edit by `write_library`.
    pub library: Library,
    /// This project's policy: the patches, and `divisions`.
    pub policy: emerge_core::policy::Policy,
    /// Per-descriptor token masks, in library order — resolved once at load so the palette and the
    /// placement rules never re-resolve the same strings.
    pub masks: Vec<Masks>,
    /// The map being edited, and where it will be written.
    pub map: Map,
    pub map_path: PathBuf,
    /// Whether there are edits not yet on disk.
    pub dirty: bool,
    /// **Descriptor ids whose resolved form changed since the Map last redrew**, and which therefore
    /// have placements standing on screen built from an older shape.
    ///
    /// Written at the one commit door (`tiles::commit_measured`) by **comparing** the rebuilt library
    /// against the one it replaces — never declared by a call site. Fifteen edit paths reach that
    /// door and a list each of them had to remember to append to is a list that goes stale on the
    /// sixteenth; a diff cannot miss one.
    ///
    /// Drained by `editor::redraw_edited`. Empty is the normal state, and checking it is one
    /// `is_empty` per frame.
    pub touched: Vec<String>,
    /// Triangles per library entry, in library order.
    ///
    /// Measured at open from each GLB's JSON chunk — accessor counts only, no vertex data — rather
    /// than stored in the descriptor. Triangle count is a fact about the *file*, so keeping it in the
    /// schema would mean a number that silently goes stale the first time an artist re-exports. This
    /// costs a few milliseconds and cannot be wrong.
    pub triangles: Vec<usize>,
}

impl Project {
    /// Read a project, or say exactly what is wrong with it.
    pub fn open(root: &Path, map_name: &str, kit: Option<&str>) -> Result<Project, String> {
        // Forced, not checked: whatever was typed on the command line becomes the one spelling before
        // anything else sees it, so there is no path through this program on which a map has a name
        // the filesystem and the schema disagree about.
        let name = naming::to_snake_case(map_name);
        if name.is_empty() {
            return Err(format!(
                "`{map_name}` leaves nothing usable as a name. Names are snake_case — lowercase \
                 letters, digits and single underscores, starting with a letter."
            ));
        }

        // The kit name is forced into a plain directory name the same way the map name is, so
        // `--kit ../../etc` cannot walk out of the project.
        let emerge_dir = match kit {
            Some(k) => {
                let k = naming::to_snake_case(k);
                if k.is_empty() {
                    return Err(format!(
                        "`{}` leaves nothing usable as a kit name. A kit is a directory under \
                         `{EMERGE_DIR}` — snake_case, like `site`.",
                        kit.unwrap_or_default()
                    ));
                }
                let dir = root.join(EMERGE_DIR).join(&k);
                if !dir.join(LIBRARY_FILE).is_file() {
                    return Err(format!(
                        "no kit `{k}`: {} has no {LIBRARY_FILE}. Kits are directories under \
                         `{EMERGE_DIR}`.",
                        dir.display()
                    ));
                }
                dir
            }
            None => root.join(EMERGE_DIR),
        };

        let vocab_path = root.join(VOCAB);
        let vocab = Vocabularies::parse(&read(&vocab_path)?)
            .map_err(|e| format!("{}: {e}", vocab_path.display()))?;

        // Measurements, then this game's policy over them — `emerge_core::policy` owns the order so
        // the editor and the game cannot end up with differently-layered libraries. All three layers
        // come back because this editor writes the bottom one and reads `divisions` off the middle.
        let emerge_core::policy::Layered {
            measured,
            library,
            policy,
            compositions,
        } = emerge_core::policy::layered_library(&emerge_dir)?;
        let library_path = emerge_dir.join(LIBRARY_FILE);

        // The two-sided pass over the whole set, at open. A prop that rests on a class nothing offers
        // can never be placed, and the moment to say so is now — not when an author wonders why a
        // shelf stays empty.
        let masks = library
            .resolve(&vocab)
            .map_err(|e| format!("{}: {e}", library_path.display()))?;

        // **And the holes, in the same breath.** A slot's `accepts` token is checked here rather than
        // in `composition::validate` because that runs inside the project loader, which reads the
        // library and the policy and never opens the vocabulary — so this is the first place both
        // halves exist at once. Same rule as every other token: an invented one is refused at open,
        // naming the composition, the member and the axis.
        vocab
            .check_slots(&compositions.compositions)
            .map_err(|e| format!("{}: {e}", emerge_dir.join("compositions.ron").display()))?;

        let map_path = emerge_dir.join(naming::map_file_name(&name));
        // A map that does not exist yet is a new map, not an error: this is how an author starts one.
        // A map that exists and does not parse IS an error — silently replacing it with an empty one
        // would destroy their work on the first save.
        let map = if map_path.is_file() {
            let loaded: Map =
                Map::parse(&read(&map_path)?).map_err(|e| format!("{}: {e}", map_path.display()))?;
            // A map's name and its file have to agree, or a rename leaves two files answering to one
            // name and nobody can say which is the level.
            if loaded.name != name {
                return Err(format!(
                    "{} calls itself `{}`. A map's name IS its file — rename the file to \
                     `{}` or open it under its own name.",
                    map_path.display(),
                    loaded.name,
                    naming::map_file_name(&loaded.name)
                ));
            }
            loaded
        } else {
            Map {
                version: MAP_VERSION,
                name: name.clone(),
                note: Some(format!("Authored in emerge-mapper. Library: {}.", library_path.display())),
                ..Map::default()
            }
        };

        // Counts come from the one census, never from a `.len()` written here — see
        // `emerge_core::census` for the drift that discipline exists to prevent.
        let catalog = emerge_core::census::of_catalog(&library, &compositions.compositions);
        let counted = emerge_core::census::of_map(&map);
        info!(
            "project: {} — {} descriptor(s), {} composition(s), {} placement(s), {} stamp(s), map {}",
            root.display(),
            catalog.descriptors,
            catalog.compositions,
            counted.placements,
            counted.stamps,
            map_path.display()
        );

        let triangles = library
            .descriptors
            .iter()
            .map(|d| {
                d.mesh
                    .as_deref()
                    .map(|m| root.join("assets").join(m))
                    .and_then(|path| emerge_core::glb::Glb::open(&path).ok())
                    .map_or(0, |g| emerge_core::import::triangles(&g))
            })
            .collect();

        Ok(Project {
            root: root.to_path_buf(),
            emerge_dir,
            library_path,
            vocab,
            measured,
            compositions,
            library,
            policy,
            masks,
            map,
            map_path,
            dirty: false,
            touched: Vec::new(),
            triangles,
        })
    }

    /// **This piece's lattice divisions**, from its own size and the project's divisions-per-tile.
    ///
    /// The one place the editor derives them, so the grid an author clicks, the grid the gizmos
    /// draw, and the grid a write is range-checked against cannot come out different.
    pub fn divisions_of(
        &self,
        d: &emerge_core::descriptor::Descriptor,
    ) -> Result<(u32, u32, u32), String> {
        emerge_core::descriptor::divisions(d, self.policy.face_bands)
    }

    /// Write the map. Atomic, so a crash mid-write cannot leave half a level on disk.
    ///
    /// Serialized by an ordinary serializer with no comment-preserving pass, because an emerge map
    /// keeps its prose in `note:` fields — see `emerge_core::map`. That is the whole reason the
    /// decision was made that way.
    pub fn save(&mut self) -> Result<(), String> {
        self.map.validate()?;
        // **The save gate has to ask the question the game will ask.** `validate` checks the map's
        // own shape and stops there; it does not expand stamps, so a map whose group no longer
        // resolves passed this door and was written with a cheerful "saved" — and then failed at
        // `FVS_EMERGE_MAP` load with "the map has holes". The editor saying saved while the game says
        // broken is the exact drift the shared `emerge-core` validation exists to prevent.
        if !self.map.stamps.is_empty() {
            emerge_core::composition::expand(
                &self.map,
                &self.map.stamps,
                &self.compositions.compositions,
                &self.library,
            )
            .map_err(|e| format!("not saved — the game could not load this: {e}"))?;
        }
        // Follow a rename. The path is derived rather than remembered, so the file a map is in is
        // always the file its name says it is.
        // Beside the kit it was authored with, not at the project root's default — a map written
        // where its library is not is a map that opens against the wrong tiles.
        self.map_path = self
            .emerge_dir
            .join(naming::map_file_name(&self.map.name));
        let text = ron::ser::to_string_pretty(&self.map, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("map: serialize: {e}"))?;
        emerge_core::ron_surgery::save_atomic(&self.map_path, &text)?;
        self.dirty = false;
        Ok(())
    }

    /// Re-measure the per-entry triangle counts after the library changes.
    ///
    /// Called when an import lands, so the palette's cost column covers the new piece rather than
    /// silently reading zero for it.
    pub fn remeasure_triangles(&mut self) {
        self.triangles = self
            .library
            .descriptors
            .iter()
            .map(|d| {
                d.mesh
                    .as_deref()
                    .map(|m| self.root.join("assets").join(m))
                    .and_then(|path| emerge_core::glb::Glb::open(&path).ok())
                    .map_or(0, |g| emerge_core::import::triangles(&g))
            })
            .collect();
    }

    /// **The one door to `compositions.ron`.** Insert or replace by id, sort, validate the whole set,
    /// write atomically, and adopt only on success.
    ///
    /// # Why it lives here and not on a tab
    ///
    /// The invariant FVS-R-15 bought is **one writer** — it was spent making the Compose tab read-only
    /// while the Map authored, and `tests/compose_is_read_only.rs` held the line by naming the tabs.
    /// Naming tabs was always a proxy: what mattered is that one function opens the file. Now that
    /// tiles are assembled on the Tiles tab *and* captured from a box on the Map, two tabs legitimately
    /// author compositions, and a per-tab rule would either forbid the feature or be quietly widened
    /// until it forbade nothing.
    ///
    /// So the rule moves to where it was always aimed: **`project.rs` is the only module that names
    /// the file**, and every author-facing verb comes through here. That is the same shape
    /// `tiles::commit_measured` has for `library.ron`, and the ratchet now asserts it directly.
    ///
    /// # Validated as a set, before anything is written
    ///
    /// `composition::validate` is whole-set because containment is a set property — a nested group
    /// that stops existing breaks its parent, not itself. Validating the *proposal* rather than the
    /// live set is what makes a refusal leave the project exactly as it was.
    pub fn commit_composition(
        &mut self,
        comp: emerge_core::composition::Composition,
    ) -> Result<(), String> {
        let mut proposed = self.compositions.clone();
        match proposed.compositions.iter().position(|c| c.id == comp.id) {
            Some(i) => proposed.compositions[i] = comp,
            None => proposed.compositions.push(comp),
        }
        // Canonical order, so one set has one encoding and a diff shows what changed rather than
        // where it was inserted.
        proposed.compositions.sort_by(|a, b| a.id.cmp(&b.id));
        emerge_core::composition::validate(&proposed.compositions, &self.library)?;

        let path = self
            .emerge_dir
            .join(emerge_core::composition::Compositions::FILE);
        let text = proposed.to_ron()?;
        emerge_core::ron_surgery::save_atomic(&path, &text)
            .map_err(|e| format!("NOT WRITTEN: {e}"))?;
        self.compositions = proposed;
        Ok(())
    }
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}
