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
/// **Where maps live** — the project's own directory, not a kit's.
///
/// They sat beside the kit that drew them, on the argument that *"a map means nothing without the
/// library that draws it"*. That was true while a project was one kit. It stopped being true the
/// moment a map could stamp a tile seating two kits' pieces: the library that draws a map is now the
/// **merge**, so filing it under one of the kits picks an arbitrary winner.
///
/// A subdirectory rather than the project root, and that is load-bearing: `.gitignore` carries
/// `assets/emerge/*.map.ron` to keep stray test maps out of git, and a map in `maps/` is not matched
/// by it.
const MAPS_DIR: &str = "maps";

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
    /// **The namespace this kit implements** — the `site` in `site/wall_corner`.
    ///
    /// Not the same question as [`Self::emerge_dir`], and that is the whole point: `assets/emerge/site/`
    /// and `assets/emerge/site_greybox/` are two directories providing the **identical** 45 `site/*`
    /// ids, so a namespace is an *interface* and a directory is one *implementation* of it. A tile
    /// authored in either belongs to `site`, because that is what a map naming `site/floor` binds to.
    ///
    /// Read from the library when its ids carry a namespace, and from the directory name when they do
    /// not. Resolved in [`Self::open`], which is where the reasoning and the refusals live.
    pub namespace: String,
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
    /// **The authoring kit's** policy — the patches and the exclusions it applies to its own pieces.
    ///
    /// Per-kit and not merged, because `Policy::apply` refuses a rule that matches nothing: a kit's
    /// patches name a kit's pieces, so layering one project-wide would refuse to open the moment two
    /// kits were bound.
    pub policy: emerge_core::policy::Policy,
    /// **The grid every kit, tile and map in this project shares** — `kits.ron`'s
    /// [`emerge_core::kits::Lattice`].
    ///
    /// Held here rather than on the map, which is where it lived for part of
    /// 2026-08-16. A map has exactly one lattice, but so does the project, and
    /// `composition::interface` takes the band count as an argument — so two maps at different
    /// counts gave the same tile two different adjacency contracts. It is also the only form the
    /// doors that author tiles can ask, since they have no map open at all.
    pub lattice: emerge_core::kits::Lattice,
    /// **Every bound kit, layered**, in `kits.ron` order.
    ///
    /// Held so an edit to the authoring kit can rebuild [`Self::library`] without re-reading the
    /// other kits off disk — `commit_measured` validates before it writes, so the file it would
    /// re-read is still the old one at the moment the merge is needed.
    pub kits: Vec<emerge_core::kits::KitLayer>,
    /// Per-descriptor token masks, in library order — resolved once at load so the palette and the
    /// placement rules never re-resolve the same strings.
    pub masks: Vec<Masks>,
    /// **The project's own directory** — `assets/emerge`, the one holding `vocab.ron`, `kits.ron`
    /// and `compositions.ron`. Not a kit; kits are its subdirectories.
    pub project_dir: PathBuf,
    /// **Where every map in this project lives** — see [`MAPS_DIR`]. Held rather than re-derived, so
    /// a rename follows the name without re-deciding which directory a map belongs to.
    pub maps_dir: PathBuf,
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
    pub fn open(root: &Path, kit: Option<&str>) -> Result<Project, String> {
        // **The binding is the project's, not a kit's.** `kits.ron` says which directory provides
        // which namespace — `site/` and `site_greybox/` both provide `site/*`, so this is a question
        // no directory can answer about itself. See `emerge_core::kits`.
        let project_dir = root.join(EMERGE_DIR);
        let kits_path = project_dir.join(emerge_core::kits::KITS_FILE);
        let kits = emerge_core::kits::Kits::parse(&read(&kits_path)?)
            .map_err(|e| format!("{}: {e}", kits_path.display()))?;

        // **`--kit` chooses where new work lands, not what the palette can show.**
        //
        // It used to select the *only* kit loaded, which is what made a tile authored in one kit
        // invisible to every map in another. Every bound kit is loaded now — that is the whole
        // feature — so this answers a smaller and more useful question: which kit an imported mesh
        // joins, and which namespace a new tile is named in.
        //
        // Forced into a plain directory name the same way the map name is, so `--kit ../../etc`
        // cannot walk out of the project.
        let authoring = match kit {
            Some(k) => {
                let forced = naming::to_snake_case(k);
                if forced.is_empty() {
                    return Err(format!(
                        "`{k}` leaves nothing usable as a kit name. A kit is a directory under \
                         `{EMERGE_DIR}` — snake_case, like `site`."
                    ));
                }
                if !kits.bind.iter().any(|b| b.dir == forced) {
                    return Err(format!(
                        "no kit `{forced}` in this project. {} binds {}. Add it there, or open one \
                         of those.",
                        kits_path.display(),
                        kits.bind
                            .iter()
                            .map(|b| format!("`{}`", b.dir))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                forced
            }
            None => kits.authoring.clone().ok_or_else(|| {
                format!(
                    "{} binds no kit, so there is nothing to open. Make one from the menu — the \
                     `+ new kit` row — and it becomes where new work lands.",
                    kits_path.display()
                )
            })?,
        };

        let vocab_path = root.join(VOCAB);
        let vocab = Vocabularies::parse(&read(&vocab_path)?)
            .map_err(|e| format!("{}: {e}", vocab_path.display()))?;

        // Every bound kit, each layered with its **own** policy, merged into the one library a map
        // resolves against — plus the project's compositions, validated against that merge because
        // it is the only library that can answer for a tile seating two kits' pieces.
        let bound = emerge_core::kits::bound_library(&project_dir, &kits)?;
        let emerge_core::kits::Bound {
            kits: layers,
            mut library,
            compositions,
        } = bound;

        // **The derived half of `effects` is settled over the whole set, at open.**
        //
        // `labels::IMPLIED_BY_KIND` says these tokens follow the kind and are not hand-authorable —
        // *"set the kind and the effect follows, which is what makes them trustworthy to read"*. That
        // was true of every row written after the rule landed and false of every row written before
        // it, because the only two places that settled were applying a suggestion and clicking a KIND
        // chip. Neither reaches a row already on disk, so a library carried both kinds of answer at
        // once: `lamp` had `["emit", "uses-electricity"]` and `desk_lamp_modern`, same kind, had
        // `["emit"]`. Reported at the keyboard 2026-08-18 as beds arriving without
        // `stamina-recharge`.
        //
        // Here rather than at the write, on the same argument `library.resolve` below makes: the
        // moment to reconcile the whole set is when the whole set is in hand. In memory only — the
        // merged library spans several kits and their files, so the correction reaches disk through
        // the ordinary `write_library` path the next time a row is saved.
        let effects_order: Vec<String> =
            vocab.effects.tokens.iter().map(|t| t.name.clone()).collect();
        let mut settled = 0usize;
        for d in &mut library.descriptors {
            let before = d.effects.clone();
            crate::labels::settle_implied_effects(d, &effects_order);
            if d.effects != before {
                settled += 1;
            }
        }
        if settled > 0 {
            info!("{settled} library row(s) had derived effects out of step with their kind");
        }

        // Present by construction: `Kits::validate` refuses an `authoring` that names no bind, and
        // the check above refuses a `--kit` that names none either.
        let layer = layers
            .iter()
            .find(|l| l.dir.file_name().is_some_and(|n| n == authoring.as_str()))
            .ok_or_else(|| {
                format!(
                    "{}: `{authoring}` is bound but was not loaded, which cannot happen — the \
                     binding and the loader disagree.",
                    kits_path.display()
                )
            })?;
        let emerge_dir = layer.dir.clone();
        let measured = layer.measured.clone();
        let policy = layer.policy.clone();
        // **The namespace comes from the binding, which was verified against the library.**
        // `Library::namespace` is what does the verifying (`kits::bound_library`), so this is the
        // one answer rather than a second derivation of it — and it is the only form that can name
        // `site` for a directory called `site_greybox`.
        let namespace = layer.namespace.clone();
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
            .map_err(|e| {
                format!(
                    "{}: {e}",
                    project_dir
                        .join(emerge_core::composition::Compositions::FILE)
                        .display()
                )
            })?;

        let maps_dir = project_dir.join(MAPS_DIR);

        // Counts come from the one census, never from a `.len()` written here — see
        // `emerge_core::census` for the drift that discipline exists to prevent.
        let catalog = emerge_core::census::of_catalog(&library, &compositions.compositions);
        info!(
            "project: {} — {} descriptor(s), {} composition(s), {} bound kit(s)",
            root.display(),
            catalog.descriptors,
            catalog.compositions,
            layers.len(),
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
            namespace,
            lattice: kits.lattice,
            kits: layers,
            library_path,
            vocab,
            measured,
            compositions,
            library,
            policy,
            masks,
            project_dir,
            maps_dir,
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
        emerge_core::descriptor::divisions(d, self.lattice.face_bands)
    }

    /// **The merged library, with the authoring kit's layer swapped for `layered`.**
    ///
    /// [`Self::library`] is every bound kit concatenated, so an edit to the kit being authored
    /// cannot simply replace it — doing that dropped every other kit's pieces out of the palette
    /// the moment a mesh was imported. Rebuilt from [`Self::kits`] rather than re-read from disk,
    /// because `commit_measured` validates *before* it writes and the file on disk is still the old
    /// one at the moment the merge is wanted.
    ///
    /// Validated on the way out, so a rename or an import that collides with another bound kit is
    /// refused here — at the commit door — rather than at the next open.
    pub fn merged_with(&self, layered: &Library) -> Result<Library, String> {
        let mut descriptors = Vec::new();
        for k in &self.kits {
            if k.dir == self.emerge_dir {
                descriptors.extend(layered.descriptors.iter().cloned());
            } else {
                descriptors.extend(k.library.descriptors.iter().cloned());
            }
        }
        let merged = Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: self.library.note.clone(),
            descriptors,
        };
        merged.validate().map_err(|e| {
            format!("{e} — another bound kit already defines it, so this edit would make every \
                     reference to that id ambiguous.")
        })?;
        Ok(merged)
    }

    /// **Which kit defines this piece** — by the library it is in, never by reading its id.
    ///
    /// The first version split the id on `/` and took the prefix. That is wrong for every kit that
    /// ships: the furniture library's 75 ids are flat (`lamp_tall`, not `furniture/lamp_tall`), so
    /// every one of them belonged to no kit, the palette filter matched nothing, and the whole
    /// selection was inert — ticking changed a label and offered exactly the same rows. Found by
    /// driving the shipped project rather than a fixture, which is the only place the flat ids are.
    ///
    /// `KitLayer::namespace` is the **bound** namespace, so it answers for a flat library too, and
    /// it survives a re-skin: `site_greybox` bound as `site` reports `site`, which is what a map
    /// naming `site/floor` means.
    pub fn kit_of(&self, id: &str) -> Option<&str> {
        self.kits
            .iter()
            .find(|k| k.library.get(id).is_some())
            .map(|k| k.namespace.as_str())
    }

    /// **Every map in this project that places `id`**, by name, in a stable order.
    ///
    /// # It used to ask the open map, which was the wrong map
    ///
    /// Removing a piece from a kit is refused while something still places it, and that guard read
    /// `project.map` — *the one map the author happened to have open*. So removing a piece used by
    /// **another** map in the same project was allowed, and the damage showed up later as that map
    /// refusing to resolve, with nothing pointing at the edit that caused it.
    ///
    /// The doors made the narrow version impossible rather than merely wrong: the Kits door has no
    /// open map to ask. Asking the project is the answer that was always correct, and it is
    /// affordable because removal is a keypress, not a frame.
    ///
    /// A map that will not parse is **named as unreadable rather than skipped**. Skipping it would
    /// mean a broken file silently withdrew its vote, which is how a guard quietly stops guarding.
    pub fn maps_that_place(&self, id: &str) -> Result<Vec<String>, String> {
        let dir = match std::fs::read_dir(&self.maps_dir) {
            Ok(d) => d,
            // No `maps/` yet is a project nobody has made a map in — no votes, not an error.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("{}: {e}", self.maps_dir.display())),
        };
        let mut paths: Vec<PathBuf> = Vec::new();
        for entry in dir {
            let path = entry.map_err(|e| format!("{}: {e}", self.maps_dir.display()))?.path();
            if path.to_string_lossy().ends_with(".map.ron") {
                paths.push(path);
            }
        }
        // Sorted, so the message an author reads names them in the same order every time.
        paths.sort();

        let mut out = Vec::new();
        for path in paths {
            let text = read(&path)?;
            let map = match Map::parse(&text) {
                Ok(m) => m,
                Err(e) => {
                    return Err(format!(
                        "{} will not parse, so it cannot be asked whether it places `{id}`: {e}",
                        path.display()
                    ));
                }
            };
            let placed = map.placements.iter().any(|p| p.descriptor == id);
            // A stamp reaches descriptors through its composition's members, so a map that only
            // stamps still votes — otherwise a piece seated inside every tile would read as unused.
            let stamped = map.stamps.iter().any(|s| {
                self.compositions
                    .compositions
                    .iter()
                    .find(|c| c.id == s.of)
                    .is_some_and(|c| {
                        c.members.iter().any(|m| match &m.body {
                            emerge_core::composition::Body::Descriptor { id: d, .. } => d == id,
                            emerge_core::composition::Body::Composition { .. }
                            | emerge_core::composition::Body::Slot { .. } => false,
                        })
                    })
            });
            if placed || stamped {
                out.push(map.name);
            }
        }
        Ok(out)
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

        // **The project's collection, not the authoring kit's.** A tile may seat `site/wall` beside
        // `lab/bench`, so it belongs to neither kit — filing it under whichever one happened to be
        // open is what made a tile authored in one kit invisible to every map in another.
        let path = self
            .project_dir
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

/// **The map being edited** — the Maps door's subject, and nothing else's.
///
/// # Why this is not on [`Project`]
///
/// It was, until the editor split into one door per entity. Four of the five doors — Kits, Tiles,
/// Compose and Rigs — author things that outlive any particular map, and a `Project` that carried
/// one forced each of them to name a map they would never read or write. Any name picked there is
/// arbitrary, and an arbitrary value that nothing reads is exactly the kind of thing that is still
/// sitting there when somebody later writes code that *does* read it.
///
/// The measurement that made the cut cheap: of ~270 map-field accesses in this crate, **234 were in
/// `editor.rs`** — the module that *is* the Maps door — and `dirty = true` was written in that one
/// file and nowhere else. The Meshes and Tiles doors commit straight to disk through
/// `tiles::commit_measured` and [`Project::commit_composition`], so the map is the only thing this
/// editor ever holds unsaved.
#[derive(Resource)]
pub struct OpenMap {
    /// The map itself.
    pub map: Map,
    /// Where it will be written.
    pub map_path: PathBuf,
    /// Whether there are edits not yet on disk.
    pub dirty: bool,
}

impl OpenMap {
    /// Open a map by **name**, or start one under that name.
    ///
    /// The name is forced into snake_case rather than checked, so there is no path through this
    /// program on which a map has a name the filesystem and the schema disagree about.
    pub fn open(project: &Project, map_name: &str) -> Result<OpenMap, String> {
        let name = naming::to_snake_case(map_name);
        if name.is_empty() {
            return Err(format!(
                "`{map_name}` leaves nothing usable as a name. Names are snake_case — lowercase \
                 letters, digits and single underscores, starting with a letter."
            ));
        }
        let map_path = project.maps_dir.join(naming::map_file_name(&name));
        // A map that does not exist yet is a new map, not an error: this is how an author starts one.
        // A map that exists and does not parse IS an error — silently replacing it with an empty one
        // would destroy their work on the first save.
        let map = if map_path.is_file() {
            let loaded: Map = Map::parse(&read(&map_path)?)
                .map_err(|e| format!("{}: {e}", map_path.display()))?;
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
                note: Some(format!(
                    "Authored in emerge-mapper. Library: {}.",
                    project.library_path.display()
                )),
                ..Map::default()
            }
        };

        let counted = emerge_core::census::of_map(&map);
        info!(
            "map: {} — {} placement(s), {} stamp(s)",
            map_path.display(),
            counted.placements,
            counted.stamps
        );

        Ok(OpenMap { map, map_path, dirty: false })
    }

    /// Write the map. Atomic, so a crash mid-write cannot leave half a level on disk.
    ///
    /// Serialized by an ordinary serializer with no comment-preserving pass, because an emerge map
    /// keeps its prose in `note:` fields — see `emerge_core::map`. That is the whole reason the
    /// decision was made that way.
    pub fn save(&mut self, project: &Project) -> Result<(), String> {
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
                &project.compositions.compositions,
                &project.library,
            )
            .map_err(|e| format!("not saved — the game could not load this: {e}"))?;
        }
        // Follow a rename. The path is derived rather than remembered, so the file a map is in is
        // always the file its name says it is.
        //
        // In the project's `maps/`, not beside a kit. It used to be beside the kit it was authored
        // with, on the argument that a map written where its library is not opens against the wrong
        // tiles — which was right while a project was one kit and is now the opposite of right: the
        // library that draws a map is the merge of every bound kit, so filing it under one of them
        // picks a winner arbitrarily.
        self.map_path = project.maps_dir.join(naming::map_file_name(&self.map.name));
        let text = ron::ser::to_string_pretty(&self.map, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("map: serialize: {e}"))?;
        emerge_core::ron_surgery::save_atomic(&self.map_path, &text)?;
        self.dirty = false;
        Ok(())
    }


    /// **The namespaces this map's content already names** — derived every time, never stored.
    ///
    /// Placements name a descriptor directly; a stamp names a composition, whose members name
    /// descriptors, so both are walked. `emerge_core::census` makes the same argument for counts and
    /// this is the same discipline: a set that could be cached is a set that can be wrong, and this
    /// one decides whether a checkbox is allowed to be unticked.
    pub fn namespaces_in_use(&self, project: &Project) -> std::collections::BTreeSet<String> {
        let mut out = std::collections::BTreeSet::new();
        let mut add = |id: &str| {
            if let Some(k) = project.kit_of(id) {
                out.insert(k.to_owned());
            }
        };
        for p in &self.map.placements {
            add(&p.descriptor);
        }
        for stamp in &self.map.stamps {
            let Some(c) = project
                .compositions
                .compositions
                .iter()
                .find(|c| c.id == stamp.of)
            else {
                continue;
            };
            for m in &c.members {
                match &m.body {
                    emerge_core::composition::Body::Descriptor { id, .. }
                    | emerge_core::composition::Body::Composition { id } => add(id.as_str()),
                    emerge_core::composition::Body::Slot { .. } => {}
                }
            }
        }
        out
    }


    /// **What the palette offers**: the author's choice, plus whatever the map already uses.
    ///
    /// [`emerge_core::map::Map::palette`] empty means *all bound kits* — the state a new map starts
    /// in, and what every map written before the field existed already meant.
    ///
    /// **The union with [`Self::namespaces_in_use`] is what makes this safe to edit.** Unticking a
    /// kit whose pieces are already on the map would otherwise hide the rows that describe them: the
    /// map would still load and still draw, and the author would be unable to find, re-place or
    /// match the pieces in front of them. Folding the in-use set back in means the checkbox cannot
    /// do damage, which is the whole reason it is allowed to be a checkbox.
    pub fn palette_namespaces(&self, project: &Project) -> std::collections::BTreeSet<String> {
        if self.map.palette.is_empty() {
            return project.kits.iter().map(|k| k.namespace.clone()).collect();
        }

        let mut out: std::collections::BTreeSet<String> =
            self.map.palette.iter().cloned().collect();
        out.extend(self.namespaces_in_use(project));
        out
    }

}
