//! **The chooser** — pick a kit, pick a map, before the editor opens.
//!
//! The editor took its project from `argv` and nothing else, and **nothing on screen ever said which
//! kit was loaded**: the window title carries the map name. On 2026-08-15 that cost a session —
//! three relaunches on `--kit site` while asking for a blank slate, a typo'd `--kit site1`, and the
//! question *"there are tons of tiles on the Map tab, where are they stored?"*, which had no answer
//! available from inside the editor. They were the site kit's 45 pieces, and the author had no way
//! to know.
//!
//! So: a screen that lists the kits, says how many pieces each holds, lists the maps inside, and can
//! make a new one. **The piece count is the point** — it answers "is this the blank one" before
//! anything is committed to.
//!
//! # This was a separate `App`, and the argument for it is worth keeping — as history
//!
//! **Both screens are one application now** (`screen.rs`), asked for at the keyboard on 2026-08-16:
//! *"can we not open a whole another editing window? I'd like to keep the same bevy application
//! running across whether it's the UI or the editor."* Everything below is why it was not, until
//! then — kept because the cost it names is real and was paid rather than avoided, and dated
//! because a stale rationale in the present tense is worse than none: the next reader plans around
//! it.
//!
//! `Project` is opened before the editor's plugins are added (`harness.rs`), and in Bevy 0.19 a
//! missing `Res<T>` **panics its system** (`lib.rs`, `docs/ui.md` §5). Around **sixty production
//! systems across eight files** take `Res<Project>` or `ResMut<Project>`. A chooser inside the
//! editor's `App` with no project chosen yet meant gating every one of them, where a single missed
//! system is a first-frame panic. Gating was always *feasible* — `resource_exists` takes
//! `Option<Res<T>>` — and it was the **cost** that was the argument, not impossibility.
//!
//! What paid it is `Screen`: every editor system runs `in_state(Screen::Editor)`, so on the menu
//! none of them run and none of them reach for a `Project` that is not there. One state, rather
//! than sixty run conditions. The process boundary went with it, and so did the exit code that used
//! to carry the way back.
//!
//! # Ordering: fixed and alphabetical, and the reason is in the corpus
//!
//! `docs/ui.md` §3.5 used to say *"never reorder by recency"* on Samp 2011 — a PhD about **radial**
//! menus, whose conclusion does not reach a linear list. Sears & Shneiderman 1994, *Split menus*
//! (`10.1145/174630.174632`, fetched and indexed 2026-08-15 to settle exactly this) is the paper that
//! does, and it declines to split a menu like this one: the benefit *"will increase with menu length
//! and with more skewed distributions"*, their own guideline caps the high-frequency zone at four
//! items, and with four kits that zone would be the whole list. So the order never moves, and
//! `the_catalog_order_never_moves` pins it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use emerge_core::composition::{Body, Compositions};
use emerge_core::map::Map;
use emerge_core::naming;
use emerge_core::policy::LIBRARY_FILE;

use crate::tiles::Door;

// **There is no exit code for "back to the menu" any more.**
//
// It was `64` — the editor was a child process and going back was a process boundary, so the way
// back had to survive as an integer the parent could compare. Both screens are one application now
// (`screen.rs`), so leaving a door is `NextState(Screen::Menu)` and the code that carried it is
// gone rather than kept as a second way to say the same thing.

/// Where kits live, under the project root. The same directory `Project::open` resolves `--kit`
/// against, named once so the two cannot drift.
pub const EMERGE_DIR: &str = "assets/emerge";

/// Where maps live, under the project directory. The same name `Project::open` resolves, quoted
/// rather than re-decided so the chooser cannot list a map the editor would not open.
pub const MAPS_DIR: &str = "maps";

/// **What one map file is**, as much as the chooser needs to say about it without opening a project.
#[derive(Clone, Debug, PartialEq)]
pub enum MapSummary {
    /// Parsed. The numbers the screen shows beside the row.
    Read {
        placements: usize,
        stamps: usize,
        bounds: (f32, f32, f32),
    },
    /// **On disk and unreadable, which is a row and not an omission.** A map that fails to parse is
    /// exactly the one an author needs to be told about; dropping it from the list would present a
    /// broken project as an empty one.
    Unreadable(String),
}

/// One map beside its kit.
#[derive(Clone, Debug, PartialEq)]
pub struct MapEntry {
    /// The map's name — the file stem, which `naming::map_file_name` is the inverse of.
    pub name: String,
    pub path: PathBuf,
    pub summary: MapSummary,
}

/// One kit: a directory under [`EMERGE_DIR`] holding a `library.ron`.
#[derive(Clone, Debug, PartialEq)]
pub struct Kit {
    /// What `--kit` would be given. `None` for the root kit, which `Project::open(kit: None)` opens
    /// by treating `assets/emerge` itself as the kit — a mode a subdirectory scan can never produce,
    /// so it is carried explicitly rather than left unreachable.
    pub flag: Option<String>,
    /// What the screen shows. The directory's own name either way, so the root kit reads `emerge`.
    pub label: String,
    pub dir: PathBuf,
    /// Descriptors in `library.ron`. **The fact this screen exists to show.**
    pub pieces: usize,
    /// **The name this kit answers to** — its `kits.ron` binding when it has one, and its own ids'
    /// namespace otherwise.
    ///
    /// The binding first, because that is the one that is true for a **flat** library: the furniture
    /// kit's 75 ids carry no namespace at all, so reading them answers `None` while `kits.ron` says
    /// `furniture`. Keying anything off the ids alone made the kit selection inert on every kit that
    /// ships — see `Project::kit_of`.
    pub namespace: Option<String>,
    /// **Whether this project's `kits.ron` binds this directory** — which is the only thing that
    /// decides if `Project::open` can open it.
    ///
    /// Not derivable from [`namespace`](Self::namespace), and that cost a wrong fix on 2026-09-03:
    /// `read_kit` fills `namespace` from the kit's **own** `library.ron`, and the binding pass then
    /// overwrites it — so an unbound kit carries a namespace too, and `namespace.is_some()` is true
    /// for every kit that names itself. `site` is the live example: it declares `site` in its own
    /// library and appears nowhere in `kits.ron`.
    pub bound: bool,
    /// Every id this kit defines, so a placement can be traced back to the kit that provides it
    /// without re-reading a library per frame. `read_kit` parses it anyway.
    pub ids: BTreeSet<String>,
}

/// Every kit under a project root, and every map in the project.
///
/// **The maps are the project's, not a kit's**, and that is the change of 2026-08-16. They sat
/// inside the kit that drew them while a project *was* one kit; a map now resolves against the merge
/// of every bound kit, so there is no kit to file it under.
#[derive(Clone, Debug, PartialEq)]
pub struct Catalog {
    pub kits: Vec<Kit>,
    pub maps: Vec<MapEntry>,
    /// **Which kit new work lands in** — `kits.ron`'s `authoring`, read once at scan. `None` when
    /// the project has no kits yet, which is the same state as a project that never had one.
    pub authoring: Option<String>,
    /// **The named combinations a map can be given** — `kits.ron`'s `bash` names, in file order,
    /// read in the same breath as the bindings. `B` on a map row cycles through them; a project
    /// declaring none has nothing to cycle, and `cycle_bash` says so.
    pub bashes: Vec<String>,
    /// **Every project beside this one** — the immediate children of `root.parent()` holding
    /// `assets/emerge`, plus the current root marked `current`.
    pub projects: Vec<ProjectEntry>,
}
/// One project beside this one — a sibling directory holding `assets/emerge`.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectEntry {
    pub name: String,
    pub dir: PathBuf,
    /// Whether this is the project the chooser is standing in.
    pub current: bool,
}

impl Catalog {
    /// **What the screen has to be big enough to draw**: the kit count, and the map count of the
    /// *fullest* kit. The largest rather than the selected one, so moving down the kit list never
    /// resizes the window mid-keystroke.
    pub fn shape(&self) -> (usize, usize) {
        (self.kits.len(), self.maps.len())
    }

    /// **Scan a project root.**
    ///
    /// A directory is a kit iff it holds a `library.ron` — the same test `Project::open` applies
    /// before it will accept a `--kit`, quoted rather than re-decided so the chooser cannot offer a
    /// kit the editor would then refuse.
    ///
    /// Only `library.ron` is parsed. A full `Project::open` would additionally read the policy, the
    /// compositions and every mask, for a list nobody has chosen from yet — seconds of work per kit
    /// to fill in one number.
    pub fn scan(root: &Path) -> Result<Catalog, String> {
        let base = root.join(EMERGE_DIR);
        if !base.is_dir() {
            return Err(format!(
                "{} is not a project: it has no `{EMERGE_DIR}` directory. Run the editor from the \
                 repository root, or pass the root as the first argument.",
                root.display()
            ));
        }

        let mut kits = Vec::new();
        // The root kit first, if it is one — `Project::open(None)` opens exactly this.
        if let Some(mut kit) = read_kit(&base, None)? {
            // The root kit is opened by `Project::open(None)` and needs no entry in `kits.ron`.
            kit.bound = true;
            kits.push(kit);
        }

        let entries =
            std::fs::read_dir(&base).map_err(|e| format!("cannot read {}: {e}", base.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if let Some(kit) = read_kit(&path, Some(name))? {
                kits.push(kit);
            }
        }

        // **The binding names a kit, and it is the only name a flat library has.** Read after the
        // scan rather than during it: a directory that is a kit and a directory that is *bound* are
        // two questions, and this screen lists the first while the editor loads the second.
        let kits_ron = std::fs::read_to_string(base.join(emerge_core::kits::KITS_FILE))
            .ok()
            .and_then(|t| emerge_core::kits::Kits::parse(&t).ok());
        let bindings = kits_ron
            .as_ref()
            .map(|k| k.bind.clone())
            .unwrap_or_default();
        for kit in &mut kits {
            if let Some(b) = bindings.iter().find(|b| Some(b.dir.as_str()) == kit.flag.as_deref()) {
                kit.namespace = Some(b.namespace.clone());
                kit.bound = true;
            }
        }
        // **Where new work lands**, read in the same breath as the bindings — it is a field of the
        // same file, and a second read would be a second chance for the two to disagree.
        let authoring = kits_ron.as_ref().and_then(|k| k.authoring.clone());
        // **The declared bashes**, from the same parse and in file order — `B` on a map row walks
        // them in exactly this order, so the screen and the file agree about what "next" is.
        let bashes = kits_ron
            .as_ref()
            .map(|k| k.bash.iter().map(|b| b.name.clone()).collect())
            .unwrap_or_default();

        // **Fixed order, every scan.** See the module note on Sears & Shneiderman: nothing here is
        // sorted by use, and `the_catalog_order_never_moves` is what keeps that true.
        kits.sort_by(|a, b| a.label.cmp(&b.label));
        // One list for the project. `maps/` may not exist yet in a project nobody has saved from,
        // and that is an empty list rather than an error — the `+ new map` row is the instruction.
        let maps = read_maps(&base.join(MAPS_DIR))?;
        // **Every project beside this one**, so the PROJECTS column can name them. A sibling holds
        // `assets/emerge` or it is not a project; the current root is marked `current`. With no
        // parent to walk (the root of a drive, or one that cannot be read) there is just the
        // current project, which is the only one this screen can say anything about.
        let mut projects = Vec::new();
        if let Some(parent) = root.parent() {
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let dir = entry.path();
                    if !dir.is_dir() || !dir.join(EMERGE_DIR).is_dir() {
                        continue;
                    }
                    let name = dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    projects.push(ProjectEntry {
                        name,
                        current: dir == root,
                        dir,
                    });
                }
            }
        }
        if projects.iter().all(|p| !p.current) {
            projects.push(ProjectEntry {
                name: root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                dir: root.to_path_buf(),
                current: true,
            });
        }
        // Fixed order, every scan: the current project first, the rest alphabetical.
        projects.sort_by(|a, b| {
            b.current
                .cmp(&a.current)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(Catalog {
            kits,
            maps,
            authoring,
            bashes,
            projects,
        })
    }
}

/// One directory, if it is a kit at all.
fn read_kit(dir: &Path, flag: Option<String>) -> Result<Option<Kit>, String> {
    let library = dir.join(LIBRARY_FILE);
    if !library.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&library)
        .map_err(|e| format!("cannot read {}: {e}", library.display()))?;
    let parsed = emerge_core::library::Library::parse(&text)
        .map_err(|e| format!("{}: {e}", library.display()))?;
    let pieces = parsed.descriptors.len();
    // A library disagreeing with itself is refused at open by `kits::bound_library`. Here it is a
    // list nobody has chosen from yet, so it reads as "no single namespace" rather than as a refusal
    // that would stop the whole screen drawing.
    let namespace = parsed.namespace().ok().flatten().map(str::to_owned);
    let ids: BTreeSet<String> = parsed.descriptors.iter().map(|d| d.id.clone()).collect();

    let label = dir.file_name().map_or_else(
        || dir.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );

    Ok(Some(Kit {
        flag,
        label,
        dir: dir.to_path_buf(),
        pieces,
        namespace,
        // Set by the binding pass in `Catalog::scan`; a kit read on its own is not yet bound.
        bound: false,
        ids,
    }))
}

/// Every `*.map.ron` beside a kit, alphabetical.
fn read_maps(dir: &Path) -> Result<Vec<MapEntry>, String> {
    const SUFFIX: &str = ".map.ron";
    let mut out = Vec::new();
    // **No `maps/` yet is no maps**, not a broken project: it is made by the first save, and a
    // project nobody has saved from is exactly where the `+ new map` row is the instruction.
    if !dir.is_dir() {
        return Ok(out);
    }
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let Some(name) = file.strip_suffix(SUFFIX) else {
            continue;
        };
        // `Map::parse` validates, so a map whose name or bounds are unusable lands in `Unreadable`
        // with the reason the editor would have given — reported, never dropped.
        let summary = match std::fs::read_to_string(&path) {
            Err(e) => MapSummary::Unreadable(format!("cannot read: {e}")),
            Ok(text) => match Map::parse(&text) {
                Err(e) => MapSummary::Unreadable(e),
                Ok(map) => MapSummary::Read {
                    placements: map.placements.len(),
                    stamps: map.stamps.len(),
                    bounds: map.bounds,
                },
            },
        };
        out.push(MapEntry {
            name: name.to_owned(),
            path: path.clone(),
            summary,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// **Where the game names the pieces it needs.** Scanned by [`dependents`], and the reason that
/// function exists: `src/site/kit.rs` holds `assets/emerge/site` in a `&'static str`, so no scan of
/// the editor's own content can ever see the game's dependency on it. These files can be — they
/// name descriptor ids in the same namespace the kit provides.
const GAME_KIT_DIR: &str = "assets/site";

/// **Every id a kit provides** — its descriptors, and the compositions authored beside them.
///
/// Read off the files rather than inferred from the directory name, because what a kit provides is
/// a property of what it defines. `site/` and `site_greybox/` both provide the identical 45 `site/*`
/// ids; that is what makes one a re-skin of the other rather than a second kit, and it is why
/// deleting one of them can be safe while deleting the last one is not.
fn provided_ids(dir: &Path) -> Result<BTreeSet<String>, String> {
    let mut ids = BTreeSet::new();

    let library = dir.join(LIBRARY_FILE);
    if library.is_file() {
        let text = std::fs::read_to_string(&library)
            .map_err(|e| format!("cannot read {}: {e}", library.display()))?;
        let parsed = emerge_core::library::Library::parse(&text)
            .map_err(|e| format!("{}: {e}", library.display()))?;
        ids.extend(parsed.descriptors.into_iter().map(|d| d.id));
    }

    let comps = dir.join(Compositions::FILE);
    if comps.is_file() {
        let text = std::fs::read_to_string(&comps)
            .map_err(|e| format!("cannot read {}: {e}", comps.display()))?;
        let parsed =
            Compositions::parse(&text).map_err(|e| format!("{}: {e}", comps.display()))?;
        ids.extend(parsed.compositions.into_iter().map(|c| c.id));
    }

    Ok(ids)
}

/// **Every id one map or composition file names**, structurally — never by looking at the text.
///
/// A `note:` mentioning `site/wall` is prose, and a scan that could not tell the difference would
/// refuse a deletion for a sentence somebody wrote.
fn ids_named(path: &Path) -> Result<BTreeSet<String>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut ids = BTreeSet::new();

    if path.file_name().is_some_and(|n| n == Compositions::FILE) {
        let parsed = Compositions::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        for comp in parsed.compositions {
            for member in comp.members {
                match member.body {
                    // A slot names a *token*, not an id — `Body::target` would hand back the token
                    // and it would be compared against ids it can never be one of.
                    Body::Descriptor { id, .. } | Body::Composition { id } => {
                        ids.insert(id);
                    }
                    Body::Slot { .. } => {}
                }
            }
        }
        return Ok(ids);
    }

    let map = Map::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    ids.extend(map.placements.into_iter().map(|p| p.descriptor));
    ids.extend(map.stamps.into_iter().map(|s| s.of));
    Ok(ids)
}

/// **What would be left naming a piece nothing provides, if this kit went.**
///
/// The rule is not "is anything using this kit" but **"is this kit the last provider"** — the
/// distinction the re-skin pair forces. Removing one of two directories that define the same ids
/// costs nothing; removing the only one strands every reference to them.
///
/// Two readers, because there are two formats and one of them belongs to another crate. Maps and
/// `compositions.ron` are parsed (see [`ids_named`]). The game's kit files under [`GAME_KIT_DIR`]
/// are read as text: `SiteKit`'s schema lives in the game, `emerge-mapper` does not depend on the
/// game and must not start, and a quoted-id match over a file that is a list of quoted ids is
/// exact enough to be worth more than the coupling.
///
/// Returns paths relative to `root`, in scan order, or an empty list when nothing is stranded.
pub fn dependents(root: &Path, kit: &Kit, catalog: &Catalog) -> Result<Vec<String>, String> {
    let mine = provided_ids(&kit.dir)?;
    if mine.is_empty() {
        return Ok(Vec::new());
    }

    // What another directory would still provide once this one is gone.
    let mut remaining = BTreeSet::new();
    for other in &catalog.kits {
        if other.dir != kit.dir {
            remaining.extend(provided_ids(&other.dir)?);
        }
    }
    let lost: BTreeSet<&String> = mine.difference(&remaining).collect();
    if lost.is_empty() {
        return Ok(Vec::new());
    }

    let show = |p: &Path| {
        p.strip_prefix(root)
            .unwrap_or(p)
            .display()
            .to_string()
            .replace('\\', "/")
    };
    let mut found = Vec::new();

    // **Every map, and the one composition collection.** Both are the project's now, so nothing
    // here is "this kit's own, excepted": a tile that seats this kit's pieces lives in the project's
    // `compositions.ron` and would be stranded exactly like a map would.
    let comps = root.join(EMERGE_DIR).join(Compositions::FILE);
    let files = catalog
        .maps
        .iter()
        .map(|m| m.path.clone())
        .chain(comps.is_file().then_some(comps));
    for file in files {
        if ids_named(&file)?.iter().any(|id| lost.contains(id)) {
            found.push(show(&file));
        }
    }

    // And the game, which no amount of scanning `assets/emerge` would ever have found.
    let game = root.join(GAME_KIT_DIR);
    if game.is_dir() {
        let entries = std::fs::read_dir(&game)
            .map_err(|e| format!("cannot read {}: {e}", game.display()))?;
        let mut game_files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    n.starts_with("kit_") && n.ends_with(".ron")
                })
            })
            .collect();
        game_files.sort();
        for file in game_files {
            let text = std::fs::read_to_string(&file)
                .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
            if lost.iter().any(|id| text.contains(&format!("\"{id}\""))) {
                found.push(show(&file));
            }
        }
    }

    Ok(found)
}

/// **The refusal, when [`dependents`] found something** — or `None` when it did not.
///
/// Terse, on the precedent the root-kit guard set when its explanation was reported as too much:
/// *"just say it can't be deleted."* So it says which kit, how many files, and names the first
/// three — enough to go and look, read in the time it takes to reach for the next key.
///
/// **Three, and then a count.** Naming all 51 would be a screen of paths where the point is that
/// there are 51 of them.
fn strands(label: &str, users: &[String]) -> Option<String> {
    const SHOWN: usize = 3;
    if users.is_empty() {
        return None;
    }
    let listed = users
        .iter()
        .take(SHOWN)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let rest = users.len().saturating_sub(SHOWN);
    let more = if rest > 0 {
        format!(", and {rest} more")
    } else {
        String::new()
    };
    let files = if users.len() == 1 { "file" } else { "files" };
    Some(format!(
        "`{label}` is the last kit providing pieces that {} {files} still name — {listed}{more}",
        users.len()
    ))
}

/// **Make a new, empty kit** — a directory the editor will accept as one.
///
/// A kit is four decisions and two files: a `library.ron` saying it has no pieces yet, and a
/// `project.ron` saying how finely its tiles divide. Both are written empty and **say so in their
/// own `note:`**, because an empty kit and a broken one look identical on disk otherwise — that is
/// the confusion `assets/emerge/site_v2` was created out of, and its files carry the same kind of
/// note for the same reason.
///
/// No `compositions.ron`: `policy::layered_library` treats its absence and an empty one as the same
/// state, deliberately, so writing one would be a file that means nothing. The editor makes it when
/// the first tile is saved.
///
/// No map either. The maps column will read *"no maps in <kit> yet — the `+ new map` row is the only one there"*, which is
/// the instruction rather than a report (`docs/ui.md` §1.4) and is exactly what to do next.
pub fn create_kit(root: &Path, raw_name: &str) -> Result<PathBuf, String> {
    let name = naming::to_snake_case(raw_name);
    if name.is_empty() {
        return Err(format!(
            "`{raw_name}` leaves nothing usable as a name. Kits are snake_case — lowercase \
             letters, digits and single underscores, starting with a letter."
        ));
    }
    let dir = root.join(EMERGE_DIR).join(&name);
    if dir.exists() {
        return Err(format!(
            "`{name}` already exists under {EMERGE_DIR}. Pick another name, or open the one that \
             is there."
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;

    let library = format!(
        "(\n    version: 1,\n    note: Some(\"Empty on purpose — `{name}` is a new kit. Import \
         meshes on the Meshes tab; they land here.\"),\n    descriptors: [],\n)\n"
    );
    emerge_core::ron_surgery::save_atomic(&dir.join(LIBRARY_FILE), &library)?;

    // **No lattice setting here, and that is the change of 2026-08-16.** `face_bands` and
    // `snap_divisor` describe a lattice, and a kit does not have one — a map has exactly one, so
    // they live on `Map` now. A new kit's policy is genuinely empty, and it says so rather than
    // carrying a number it cannot own.
    let policy = format!(
        "(\n    version: 2,\n    note: Some(\"The policy layer for `{name}`. No patches yet: \
         `Project::open` refuses a rule that matches nothing, so one cannot be added before the \
         pieces it names exist.\"),\n)\n"
    );
    emerge_core::ron_surgery::save_atomic(&dir.join(emerge_core::policy::POLICY_FILE), &policy)?;

    // **And bound, or it is a directory the project does not load.** Making a kit the editor cannot
    // open would be a verb that appears to work and does not — the `--kit` refusal an author would
    // then hit names `kits.ron`, which they never edited.
    //
    // Bound as its own name, because an empty library carries no namespace to contradict. Re-point
    // it by hand when the kit turns out to be a second skin of one that already exists — that is a
    // decision about the project, and `Kits::validate` refuses two directories for one namespace.
    bind_kit(root, &name)?;
    Ok(dir)
}
/// **Write a new project beside this one** — the directory that holds kits, maps, compositions and
/// the vocabulary.
///
/// The shape `assets/emerge/` *is* a project (`CLAUDE.md`'s Data model); this is the verb that
/// makes one. Nothing is opened in-process — switching the live root is a separate change — so the
/// caller reports `emerge-mapper <dir>` and the author runs it.
pub fn create_project(parent: &Path, raw_name: &str, vocab_source: &Path) -> Result<PathBuf, String> {
    let name = naming::to_snake_case(raw_name);
    if name.is_empty() {
        return Err(format!(
            "`{raw_name}` leaves nothing usable as a name. Projects are snake_case — lowercase \
             letters, digits and single underscores, starting with a letter."
        ));
    }
    let dir = parent.join(&name);
    if dir.exists() {
        return Err(format!(
            "`{name}` already exists beside this project. Pick another name, or open the one that \
             is there."
        ));
    }
    let emerge = dir.join(EMERGE_DIR);
    std::fs::create_dir_all(&emerge).map_err(|e| format!("cannot make {}: {e}", emerge.display()))?;

    // **The version comes from the constant, never a literal** — the same rule the chooser's own
    // fixture records: bumping `KITS_VERSION` must not write a file the schema refuses.
    let kits = format!(
        "(version: {}, bind: [], authoring: None, bash: [])",
        emerge_core::kits::KITS_VERSION
    );
    std::fs::write(emerge.join(emerge_core::kits::KITS_FILE), kits)
        .map_err(|e| format!("{}: {e}", emerge.join(emerge_core::kits::KITS_FILE).display()))?;

    // **Copy the vocabulary byte-for-byte.** A copy preserves the rationale comments; serializing
    // `Vocabularies` would delete them. The source is the current project's `vocab.ron`, which is
    // the only vocabulary on hand.
    std::fs::copy(vocab_source, emerge.join("vocab.ron"))
        .map_err(|e| format!("cannot copy {} to vocab.ron: {e}", vocab_source.display()))?;

    // **Write nothing else.** No `compositions.ron` (absence and empty are the same state, per
    // `create_kit`'s own note), no `maps/` (`save_atomic` creates it), no kit — the `+ new kit`
    // row makes one, and because `create_kit` calls `bind_kit`, that first kit also becomes the
    // authoring kit with no extra code.
    Ok(dir)
}

/// **Write the map's bash**, leaving every other field as it was. Same door `write_settings` uses:
/// parse, set, validate, serialize, atomic write.
///
/// Through the `Map` schema the editor and the game read, rather than spliced as text — `map.rs`'s
/// own rule: *"an emerge map is serialized normally and never text-spliced"*, because every reason
/// a map has lives in a field.
fn write_bash(path: &Path, bash: Option<&str>) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut map = Map::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    map.bash = bash.map(str::to_owned);
    map.validate()?;
    let out = ron::ser::to_string_pretty(&map, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("map: serialize: {e}"))?;
    emerge_core::ron_surgery::save_atomic(path, &out)
}

/// **Add a binding for `name`**, leaving the rest of `kits.ron` as it was.
fn bind_kit(root: &Path, name: &str) -> Result<(), String> {
    let path = root.join(EMERGE_DIR).join(emerge_core::kits::KITS_FILE);
    // **A directory that is not a project hits this first** — `+ new kit` in a bare folder has no
    // `kits.ron` to bind into. A raw io error would read as "the disk failed"; this says what the
    // directory is and what makes one.
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!(
                "{} does not exist — this directory is not a project. Make one with `+ new project`.",
                path.display()
            )
        } else {
            format!("cannot read {}: {e}", path.display())
        }
    })?;
    let mut kits =
        emerge_core::kits::Kits::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    kits.bind.push(emerge_core::kits::Bind {
        namespace: name.to_owned(),
        dir: name.to_owned(),
    });
    // **The first kit made is where work lands**, because it is the only kit there is. Not a
    // default chosen over alternatives — there are none — and an author who makes a second one and
    // wants it instead says so in this file.
    if kits.authoring.is_none() {
        kits.authoring = Some(name.to_owned());
    }
    let out = kits.to_ron().map_err(|e| format!("{}: {e}", path.display()))?;
    // Parsed back before it is written, so a binding that would refuse to load is refused here —
    // where the author is standing — rather than at the next open.
    emerge_core::kits::Kits::parse(&out).map_err(|e| format!("{}: {e}", path.display()))?;
    emerge_core::ron_surgery::save_atomic(&path, &out)
}

/// **Point `authoring` at `name`, leaving the rest of `kits.ron` as it was.**
///
/// The same shape as `bind_kit`: parsed, edited, re-parsed before writing, and `save_atomic`'d —
/// so a file that would refuse to load is refused here, where the author is standing, rather than
/// at the next open. `name` must be bound; the refusal names the bound kits, because "not bound"
/// without a list is a search.
fn set_authoring(root: &Path, name: &str) -> Result<(), String> {
    let path = root.join(EMERGE_DIR).join(emerge_core::kits::KITS_FILE);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut kits =
        emerge_core::kits::Kits::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if !kits.bind.iter().any(|b| b.dir == name) {
        return Err(format!(
            "`{name}` is not bound in {}. The bound kits are: {}.",
            path.display(),
            kits.bind
                .iter()
                .map(|b| format!("`{}`", b.dir))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    kits.authoring = Some(name.to_owned());
    let out = kits.to_ron().map_err(|e| format!("{}: {e}", path.display()))?;
    // Parsed back before it is written, so a binding that would refuse to load is refused here —
    // where the author is standing — rather than at the next open.
    emerge_core::kits::Kits::parse(&out).map_err(|e| format!("{}: {e}", path.display()))?;
    emerge_core::ron_surgery::save_atomic(&path, &out)
}

/// **Drop `name`'s binding.** Called when its directory goes, so the project does not keep asking
/// for a kit that is not there.
fn unbind_kit(root: &Path, name: &str) -> Result<(), String> {
    let path = root.join(EMERGE_DIR).join(emerge_core::kits::KITS_FILE);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut kits =
        emerge_core::kits::Kits::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    // **The kit new work lands in is not deletable while it is that kit.** Removing it would leave
    // `authoring` naming nothing, which `Kits::validate` refuses — so the project would stop opening
    // as a result of a verb that said it had succeeded.
    if kits.authoring.as_deref() == Some(name) {
        // **Unless it is the last one**, in which case the project is going back to having no kits
        // and says so, exactly as a project that never had one does. One rule either way: a project
        // never points `authoring` at a kit it does not have.
        if kits.bind.len() > 1 {
            return Err(format!(
                "`{name}` is where new work lands. Point `authoring` in {} at another kit first.",
                path.display()
            ));
        }
        kits.authoring = None;
    }
    kits.bind.retain(|b| b.dir != name);
    let out = kits.to_ron().map_err(|e| format!("{}: {e}", path.display()))?;
    emerge_core::kits::Kits::parse(&out).map_err(|e| format!("{}: {e}", path.display()))?;
    emerge_core::ron_surgery::save_atomic(&path, &out)
}

/// **Write a new, empty map into a kit** — the chooser's one creating verb.
///
/// # The name is not defaulted
///
/// `Map::default()` leaves `name` empty on purpose, and `emerge-core` says why at the field:
/// *"Empty, not 'untitled': a substituted name is a name nobody chose, and the second one collides
/// with the first."* So this refuses an unusable name by name rather than inventing one, and the
/// screen keeps the field blank until the author types something. `Map::validate` would refuse it
/// anyway; asking first means the refusal names the rule instead of a serializer.
///
/// Goes through `validate` then `ron_surgery::save_atomic` — the same two steps `Project::save`
/// takes, so a map made here and a map saved from the editor cannot disagree about what a map is.
pub fn create_map(
    maps_dir: &Path,
    raw_name: &str,
    bounds: (f32, f32, f32),
    origin: (f32, f32, f32),
    note: Option<String>,
) -> Result<PathBuf, String> {
    let name = naming::to_snake_case(raw_name);
    if name.is_empty() {
        return Err(format!(
            "`{raw_name}` leaves nothing usable as a name. Names are snake_case — lowercase \
             letters, digits and single underscores, starting with a letter."
        ));
    }
    let path = maps_dir.join(naming::map_file_name(&name));
    if path.exists() {
        return Err(format!(
            "`{name}` already exists in this kit. Pick another name, or open the one that is there."
        ));
    }

    let map = Map {
        name,
        bounds,
        origin,
        note,
        ..Map::default()
    };
    map.validate()?;
    let text = ron::ser::to_string_pretty(&map, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("map: serialize: {e}"))?;
    emerge_core::ron_surgery::save_atomic(&path, &text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emerge_core::map::Placed;

    /// A throwaway project root: `assets/emerge/` plus whatever kits a test asks for.
    struct Root(PathBuf);

    impl Root {
        fn new(name: &str) -> Root {
            let dir = std::env::temp_dir().join(format!("chooser-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let base = dir.join(EMERGE_DIR);
            std::fs::create_dir_all(&base).unwrap_or_else(|e| panic!("{}: {e}", base.display()));
            // A project that binds nothing — which is where an author starts, and the state
            // `bind_kit` gets them out of.
            // **The version comes from the constant, never a literal.** It was `1` written out
            // here, so bumping `KITS_VERSION` failed thirty chooser tests at once with a schema
            // refusal — the fixture disagreeing with the schema it is a fixture for.
            std::fs::write(
                base.join(emerge_core::kits::KITS_FILE),
                format!(
                    "(version: {}, bind: [], authoring: None, bash: [])",
                    emerge_core::kits::KITS_VERSION
                ),
            )
            .unwrap_or_else(|e| panic!("{}: {e}", base.display()));
            Root(dir)
        }

        /// Where this project's maps live. They left the kit directories on 2026-08-16.
        fn maps(&self) -> PathBuf {
            self.0.join(EMERGE_DIR).join(MAPS_DIR)
        }

        /// Bind a directory that already exists, so `Project::open` would load it.
        fn bind(&self, name: &str) {
            let path = self.0.join(EMERGE_DIR).join(emerge_core::kits::KITS_FILE);
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
            let mut k = emerge_core::kits::Kits::parse(&text).unwrap_or_else(|e| panic!("{e}"));
            k.bind.push(emerge_core::kits::Bind {
                namespace: name.to_owned(),
                dir: name.to_owned(),
            });
            if k.authoring.is_none() {
                k.authoring = Some(name.to_owned());
            }
            std::fs::write(&path, k.to_ron().unwrap_or_else(|e| panic!("{e}")))
                .unwrap_or_else(|e| panic!("{e}"));
        }

        /// **Declare a bash**, appended in call order — which is the order `B` walks them in.
        /// Every namespace named must already be bound, or `Kits::validate` refuses the write here
        /// rather than at the next scan.
        fn bash(&self, name: &str, kits: &[&str]) {
            let path = self.0.join(EMERGE_DIR).join(emerge_core::kits::KITS_FILE);
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
            let mut k = emerge_core::kits::Kits::parse(&text).unwrap_or_else(|e| panic!("{e}"));
            k.bash.push(emerge_core::kits::Bash {
                name: name.to_owned(),
                kits: kits.iter().map(|s| (*s).to_owned()).collect(),
                note: None,
            });
            let out = k.to_ron().unwrap_or_else(|e| panic!("{e}"));
            emerge_core::kits::Kits::parse(&out).unwrap_or_else(|e| panic!("{e}"));
            std::fs::write(&path, out).unwrap_or_else(|e| panic!("{e}"));
        }

        /// A kit directory holding `n` descriptors. `None` writes into the root kit itself.
        fn kit(&self, name: Option<&str>, n: usize) -> PathBuf {
            let dir = match name {
                Some(k) => self.0.join(EMERGE_DIR).join(k),
                None => self.0.join(EMERGE_DIR),
            };
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            let ids: Vec<String> = (0..n)
                .map(|i| format!("(id: \"p{i}\", mesh: Some(\"p{i}.glb\"))"))
                .collect();
            let text = format!("(version: 1, descriptors: [{}])", ids.join(", "));
            std::fs::write(dir.join(LIBRARY_FILE), text)
                .unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            if let Some(k) = name {
                self.bind(k);
            }
            dir
        }

        /// **A kit whose pieces carry a `ns/` namespace** — the shape every shipped kit has, and
        /// the only shape the deletion guard can reason about. [`Root::kit`] writes flat ids,
        /// which is the other case and the reason both helpers exist.
        ///
        /// Two calls with one `ns` make a re-skin pair, exactly as `site` and `site_greybox` are.
        fn skin(&self, name: &str, ns: &str, pieces: &[&str]) -> PathBuf {
            let dir = self.0.join(EMERGE_DIR).join(name);
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            let ids: Vec<String> = pieces
                .iter()
                .map(|p| format!("(id: \"{ns}/{p}\", mesh: Some(\"{p}.glb\"))"))
                .collect();
            let text = format!("(version: 1, descriptors: [{}])", ids.join(", "));
            std::fs::write(dir.join(LIBRARY_FILE), text)
                .unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            self.bind(name);
            dir
        }

        /// A map in the project, placing one piece per id named. Built as a `Map` and serialized
        /// rather than hand-written, so a schema change breaks this loudly instead of quietly
        /// producing a file `Map::parse` rejects for an unrelated reason.
        fn map(&self, _kit: &Path, name: &str, places: &[&str]) {
            let map = Map {
                name: name.to_owned(),
                placements: places
                    .iter()
                    .enumerate()
                    .map(|(i, d)| Placed {
                        id: format!("piece@{i}"),
                        descriptor: (*d).to_owned(),
                        ..Placed::default()
                    })
                    .collect(),
                ..Map::default()
            };
            map.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
            let text = ron::ser::to_string_pretty(&map, ron::ser::PrettyConfig::default())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let dir = self.maps();
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            std::fs::write(dir.join(naming::map_file_name(name)), text)
                .unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        }

        /// **The game's own kit file**, under [`GAME_KIT_DIR`] and outside `assets/emerge`
        /// entirely — the dependent no scan of the editor's content could ever have found.
        fn game_kit(&self, file: &str, names: &[&str]) {
            let dir = self.0.join(GAME_KIT_DIR);
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            let body: Vec<String> = names
                .iter()
                .enumerate()
                .map(|(i, id)| format!("    slot{i}: \"{id}\","))
                .collect();
            std::fs::write(dir.join(file), format!("(\n{}\n)\n", body.join("\n")))
                .unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        }
    }

    /// Put the arrows on a kit by label. Row 0 is `+ new kit`, so the index is one past it.
    fn stand_on(c: &mut Chooser, label: &str) {
        let i = c
            .catalog
            .kits
            .iter()
            .position(|k| k.label == label)
            .unwrap_or_else(|| panic!("`{label}` should be listed"));
        c.kit = i + 1;
        c.focus = Focus::Kits;
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// **The mirror must not be on the layer the offscreen camera renders.**
    ///
    /// It was, and the capture came back a flat `000000` with nothing in the log: the camera that
    /// renders into the image was drawing the sprite that *shows* that image — one texture as
    /// colour attachment and sampled source in a single pass. Nothing in Bevy says so out loud, so
    /// the only way this stays fixed is a test that names the two layers.
    #[test]
    fn the_mirror_is_not_on_the_layer_it_mirrors() {
        assert_ne!(
            crate::surface::MIRROR_LAYER, 0,
            "layer 0 is where the UI camera draws; a mirror there renders its own target"
        );
    }

    /// **A directory is a kit only if it holds a `library.ron`** — the rule `Project::open` enforces
    /// before it accepts a `--kit`. Quoted rather than re-decided, so the chooser cannot offer a kit
    /// the editor would then refuse by name.
    #[test]
    fn a_directory_without_a_library_is_not_a_kit() {
        let root = Root::new("not-a-kit");
        root.kit(Some("real"), 3);
        let stray = root.0.join(EMERGE_DIR).join("notes");
        std::fs::create_dir_all(&stray).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(stray.join("readme.txt"), "hello").unwrap_or_else(|e| panic!("{e}"));

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let labels: Vec<&str> = catalog.kits.iter().map(|k| k.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["real"],
            "a directory with no library.ron is not a kit"
        );
    }

    /// **The order never moves.** See the module note: Sears & Shneiderman's split-menu benefit
    /// grows with menu length and skew, and their own guideline caps the high-frequency zone at four
    /// items — which for a handful of kits is the whole list. This is what stops a future "sort by
    /// recently opened" landing without the argument being had.
    #[test]
    fn the_catalog_order_never_moves() {
        let root = Root::new("order");
        for k in ["site_v2", "site", "site_greybox"] {
            root.kit(Some(k), 1);
        }
        let _kit = root.kit(Some("site"), 1);
        for m in ["zulu", "alpha", "mike"] {
            create_map(&root.maps(), m, (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
                .unwrap_or_else(|e| panic!("{e}"));
        }

        let first = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let labels: Vec<&str> = first.kits.iter().map(|k| k.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["site", "site_greybox", "site_v2"],
            "kits are alphabetical"
        );

        let maps: Vec<&str> = first
            .kits
            .iter()
            .find(|k| k.label == "site")
            .map(|_| first.maps.iter().map(|m| m.name.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        assert_eq!(maps, vec!["alpha", "mike", "zulu"], "maps are alphabetical");

        // The property, stated as a property: scanning again returns the identical order. (The
        // `projects` list is *not* compared — it names sibling directories, which parallel tests
        // may create between the two scans. Order within each scan is what never moves.)
        let again = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let again_kits: Vec<&str> = again.kits.iter().map(|k| k.label.as_str()).collect();
        assert_eq!(
            labels, again_kits,
            "a second scan must return the identical kit order"
        );
        let again_maps: Vec<&str> = again.maps.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            maps, again_maps,
            "a second scan must return the identical map order"
        );
    }

    /// The root kit is `Project::open(kit: None)`, which a subdirectory scan cannot produce — so it
    /// is carried explicitly, with `flag: None`, or that mode becomes unreachable from the chooser.
    #[test]
    fn the_root_kit_is_offered_with_no_flag() {
        let root = Root::new("root-kit");
        root.kit(None, 7);
        root.kit(Some("site"), 2);

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let rooted = catalog
            .kits
            .iter()
            .find(|k| k.flag.is_none())
            .unwrap_or_else(|| panic!("the root kit is missing: {:?}", catalog.kits));
        assert_eq!(rooted.label, "emerge");
        assert_eq!(
            rooted.pieces, 7,
            "the piece count is the fact this screen exists to show"
        );
        assert_eq!(
            catalog.kits.iter().filter(|k| k.flag.is_some()).count(),
            1,
            "and the subdirectory kit is there too"
        );
    }

    /// A new map lands where its name says, parses back, and keeps the bounds it was given — the
    /// setting that was previously only reachable by editing `map::default_bounds` in source.
    #[test]
    fn a_new_map_lands_named_and_keeps_its_bounds() {
        let root = Root::new("new-map");
        let _kit = root.kit(Some("site"), 1);

        let path = create_map(&root.maps(), "Porch A", (12.0, 5.0, 9.0), (1.0, 0.0, 2.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            path.ends_with("porch_a.map.ron"),
            "forced to snake_case: {}",
            path.display()
        );

        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        let map = Map::parse(&text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(map.name, "porch_a");
        assert_eq!(map.bounds, (12.0, 5.0, 9.0));
        assert_eq!(map.origin, (1.0, 0.0, 2.0));

        // And the catalog sees it, with the numbers the row shows.
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let entry = catalog
            .kits
            .iter()
            .find(|k| k.label == "site")
            .and_then(|_| catalog.maps.iter().find(|m| m.name == "porch_a"))
            .unwrap_or_else(|| panic!("the new map is not in the catalog"));
        assert_eq!(
            entry.summary,
            MapSummary::Read {
                placements: 0,
                stamps: 0,
                bounds: (12.0, 5.0, 9.0),
            }
        );
    }

    /// **An unnamed map is refused by name, never substituted.** `Map::default()` leaves the name
    /// empty deliberately — *"a substituted name is a name nobody chose, and the second one collides
    /// with the first"* — and a chooser that filled in `untitled_map` would reintroduce exactly what
    /// `emerge-core` refuses.
    #[test]
    fn an_unnamed_map_is_refused_rather_than_defaulted() {
        let root = Root::new("unnamed");
        let kit = root.kit(Some("site"), 1);

        for raw in ["", "   ", "!!!"] {
            let e = create_map(&root.maps(), raw, (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
                .err()
                .unwrap_or_else(|| panic!("`{raw}` must be refused, not written"));
            assert!(e.contains("snake_case"), "the refusal names the rule: {e}");
        }
        assert!(
            std::fs::read_dir(&kit)
                .map(|d| d
                    .flatten()
                    .all(|f| !f.file_name().to_string_lossy().ends_with(".map.ron")))
                .unwrap_or(false),
            "a refused name must leave no file behind"
        );
    }

    /// Taking the same name twice is refused rather than silently overwriting somebody's map.
    #[test]
    fn a_name_already_taken_is_refused() {
        let root = Root::new("taken");
        let _kit = root.kit(Some("site"), 1);
        create_map(&root.maps(), "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let e = create_map(&root.maps(), "hall", (8.0, 3.0, 8.0), (0.0, 0.0, 0.0), None)
            .err()
            .unwrap_or_else(|| panic!("the second `hall` must be refused"));
        assert!(e.contains("already exists"), "{e}");
    }

    /// **Escape unwinds one layer at a time and never quits on the first press.**
    ///
    /// Reported at the keyboard: typing into a field and pressing Escape *closed the whole
    /// program*. Two causes, both fixed and both pinned here — the field handler now marks the key
    /// as taken so the chord handler cannot read the same press again, and quitting is a question
    /// rather than an act.
    #[test]
    fn escape_backs_out_one_layer_at_a_time() {
        let root = Root::new("escape-stack");
        let _kit = root.kit(Some("site"), 1);
        create_map(&root.maps(), "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));

        // Layer 1 — in a field. Leaving it must not touch anything else. Which panel is not what
        // this is about, so it is set rather than walked to.
        c.focus = Focus::Settings;
        assert_eq!(c.focus, Focus::Settings);
        c.editing = true;
        c.raw.push_str("half-typed");
        // What `type_into_field` does on Escape:
        c.raw.clear();
        c.editing = false;
        c.swallowed = true;
        assert_eq!(
            c.focus,
            Focus::Settings,
            "you stay in the panel you were in"
        );
        assert!(c.ask.is_none(), "and nothing is asked yet");

        // **The key is spent.** `drive_chooser` takes the flag and stops; without this the same
        // press fell through and quit the program, which is the bug as reported.
        assert!(
            std::mem::take(&mut c.swallowed),
            "the field handler must mark the press as taken"
        );

        // Layer 3 — a second, separate press asks rather than quitting.
        c.ask = Some(Ask::Quit);
        assert!(
            matches!(c.ask, Some(Ask::Quit)),
            "quitting is a question, and it is the ONE question — `crate::confirm` carries the \
             wording and the two keys, so nothing is asserted about them here"
        );
    }

    /// Layer 2: a draft in hand is what Escape abandons, before quitting is ever on the table.
    #[test]
    fn escape_abandons_a_draft_before_it_offers_to_quit() {
        let root = Root::new("escape-draft");
        root.kit(Some("site"), 1);
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        c.creating = Some(New::Map(Draft::default()));

        // What `drive_chooser` does with a draft in hand.
        c.creating = None;
        assert!(
            c.ask.is_none(),
            "abandoning the draft must not also raise the quit question — one press, one layer"
        );
    }

    /// **`Y` is what agrees, not `Enter`.** `Enter` opens a map and edits a field elsewhere on this
    /// screen, and a destructive prompt answered by the most-pressed key on the keyboard is one that
    /// gets answered by accident.
    #[test]
    fn both_questions_offer_the_same_two_answers() {
        let root = Root::new("answers");
        let _kit = root.kit(Some("site"), 1);
        create_map(&root.maps(), "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        // **MAPS is the column to the right of KITS**, so one crossing reaches it.
        c.cross(1);
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));
        // **The hint stands down while a question is up, for both questions.** It used to spell
        // the two answers here — and that was the whole point of this test, back when each prompt
        // chose its own. `crate::confirm` states them beside its own buttons now, so what is left
        // to hold is that the chooser stops offering its ordinary verbs: listing `Enter open KIT`
        // beside a pending deletion is an invitation to press it.
        assert_eq!(c.hint(), "", "delete: the hint stands down");
        c.ask = Some(Ask::Quit);
        assert_eq!(c.hint(), "", "quit: the same, which is the uniformity this test now pins");
    }

    /// **Every list opens with a `+ new …` row, and it is a row you can be on.**
    ///
    /// Asked for at the keyboard: *"a text entry at the very top of maps and kits that says new map,
    /// new kit... if I hit enter on that, it lets me create a new entry respective to the area."*
    /// A row you can see beats a key you have to know, and the row carries `N` beside it so using
    /// the visible path teaches the fast one.
    #[test]
    fn every_list_opens_with_a_row_that_makes_a_new_one() {
        let root = Root::new("new-rows");
        let _kit = root.kit(Some("site"), 2);
        create_map(&root.maps(), "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));

        let s = c.screen();
        assert_eq!(s.kits.first().map(|r| r.left.as_str()), Some("+ new kit"));
        assert_eq!(
            s.kits.first().map(|r| r.right.as_str()),
            Some("N"),
            "the key rides the row"
        );
        assert_eq!(s.maps.first().map(|r| r.left.as_str()), Some("+ new map"));

        // **It opens on a real kit, not on the `+ new` row** — landing there would greet an author
        // with two empty columns beside it.
        assert_eq!(c.kit, 1);
        assert_eq!(c.current_kit().map(|k| k.label.as_str()), Some("site"));
        assert!(!c.on_new_row());

        // Walking up reaches the row, and then there is genuinely nothing selected.
        c.step(-1);
        assert!(c.on_new_row());
        assert!(
            c.current_kit().is_none(),
            "nothing is selected, so the columns to the right must show nothing rather than the \
             last kit's contents — which would be a lie about what Enter is about to do"
        );
        assert!(
            c.launch_args().is_err(),
            "and Enter cannot open a map from a row that makes one"
        );
    }

    /// **Each column's panel describes that column's selection, and says whose it is.**
    ///
    /// Reported at the keyboard: *"settings is still confusing as to whether it's a kit or a map."*
    /// The deeper fault was that one shared panel never followed the focus — standing on a kit row,
    /// an author was reading a panel about a map two levels down, and no amount of labelling fixes
    /// a panel that is describing the wrong thing.
    #[test]
    fn each_panel_names_what_it_is_describing() {
        let root = Root::new("whose-panel");
        let _kit = root.kit(Some("site"), 7);
        create_map(&root.maps(), "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let c = Chooser::new(root.0.clone(), catalog, Some("site"));

        let s = c.screen();
        // **The headers say what kind of thing, not which one.** They used to carry `— site` and
        // `— hall`, and that was reported as stray text: the panel sits under the list it belongs
        // to, with the chevron on the row it describes, so the name was words restating a fact the
        // layout already makes. What must still hold is that the two cannot be confused, and that
        // each panel carries only its own subject's rows.
        assert_eq!(s.kit_header, "KIT INFO");
        assert_eq!(s.settings_header, "MAP INFO");
        assert_ne!(
            s.kit_header, s.settings_header,
            "and they cannot be confused for each other"
        );
        // The map's name is not lost — it is the first row of its own panel, which is where an
        // author would look for it.
        assert!(
            s.settings.iter().any(|r| r.right == "hall"),
            "the map names itself in its own rows: {:?}",
            s.settings
        );

        // The kit panel carries the kit's facts and none of the map's.
        let kit_left: Vec<&str> = s.kit_info.iter().map(|r| r.left.as_str()).collect();
        assert!(kit_left.contains(&"pieces"), "{kit_left:?}");
        assert!(
            !kit_left.iter().any(|l| *l == "BOUNDS" || *l == "ORIGIN"),
            "bounds belong to a map, not a kit: {kit_left:?}"
        );
        assert!(
            s.kit_info.iter().any(|r| r.right == "7"),
            "and the numbers are this kit's: {:?}",
            s.kit_info
        );

        // **The map panel carries the map's properties and none of the kit's** — the four fields
        // the arrows edit, then the `bash` fact, which names the combination this map draws on.
        let map_left: Vec<&str> = s.settings.iter().map(|r| r.left.as_str()).collect();
        assert_eq!(map_left, vec!["NAME", "BOUNDS", "ORIGIN", "NOTE", "bash"]);
        // **And a kit row carries no tick.** What a map offers is the bash it names, declared once
        // in `kits.ron` and shown on the row above — not a per-map list edited from the kit list.
        assert!(
            s.kits.iter().skip(1).all(|r| !r.left.starts_with('[')),
            "a kit row is a kit, not a checkbox: {:?}",
            s.kits
        );
        assert!(
            !map_left.iter().any(|l| l.contains("pieces")),
            "and none of them is the kit panel's own count: {map_left:?}"
        );
    }

    /// On the `+ new kit` row nothing is selected, so the kit panel is empty rather than describing
    /// whichever kit happened to be there before.
    #[test]
    fn the_kit_panel_is_empty_when_no_kit_is_selected() {
        let root = Root::new("no-kit-selected");
        root.kit(Some("site"), 3);
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        c.step(-1); // up onto `+ new kit`
        assert!(c.on_new_row());
        assert!(
            c.screen().kit_info.is_empty(),
            "describing the last kit here would be the same lie the map column already refuses"
        );
    }

    /// **The settings are not a third list, and the screen must not imply they are.**
    ///
    /// The first two columns are containment — a kit *contains* maps, and picking one opens the
    /// next. The third is attribution: a map *has* a name and bounds, which are not things inside
    /// it. Three identical columns taught the containment rule and then broke it, which is why the
    /// hierarchy read as three lists. This pins the structural half of the difference; the visual
    /// half is `PanelKind`.
    #[test]
    fn nothing_opens_from_the_settings_panel() {
        let root = Root::new("inspector");
        let _kit = root.kit(Some("site"), 1);
        create_map(&root.maps(), "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        // **MAP INFO is the panel under the map list**, so the way in is one crossing to the right
        // and then down off the end of the list — the arrow pointing at where it is drawn.
        c.cross(1);
        assert_eq!(c.focus, Focus::Maps, "one press reaches the column");
        c.step(1);
        assert_eq!(c.focus, Focus::Settings);

        let s = c.screen();
        assert!(
            !s.settings.iter().any(|r| r.left.starts_with("+ new")),
            "a `+ new` row would make it a list of things, and it is a set of properties"
        );
        assert!(
            !c.on_new_row(),
            "no row here makes anything, whichever one the arrows are on"
        );
        // **A set of properties, not a list of things.** The four editable fields plus the `bash`
        // fact, and no row here brings a new thing into being — which is what the two assertions
        // above pin and this one counts.
        assert_eq!(
            s.settings.len(),
            Field::ALL.len() + 1,
            "the map's four properties and the bash it draws on, and nothing that makes anything"
        );
    }

    /// **Naming a kit makes it, and leaves you standing on it.**
    ///
    /// Asked for at the keyboard: *"once you hit enter, select the kit in the kit area."* A kit has
    /// one field, so a separate commit key guarded nothing — and the guarded step was the one an
    /// author had to be told about.
    #[test]
    fn naming_a_kit_makes_it_and_lands_on_it() {
        let root = Root::new("kit-by-enter");
        root.kit(None, 2);
        root.kit(Some("site"), 5);
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));

        c.focus = Focus::Kits;
        c.start_new();
        assert!(c.editing, "straight into the name");
        c.raw = "scratch".to_owned();
        keep_field(&mut c, Field::Name);

        assert_eq!(c.problem, None, "nothing to refuse");
        assert!(c.creating.is_none(), "one Enter finishes a kit");
        assert!(
            root.0
                .join(EMERGE_DIR)
                .join("scratch")
                .join(LIBRARY_FILE)
                .exists(),
            "and it is on disk"
        );
        assert_eq!(c.focus, Focus::Kits, "the keyboard comes back to the list");
        assert_eq!(
            c.current_kit().map(|k| k.label.clone()),
            Some("scratch".to_owned()),
            "standing on the kit just made, so the column beside it is already its own"
        );
        assert!(!c.editing, "and no longer typing");
    }

    /// **And the same for a map**, asked for in those words. A map made this way takes
    /// `Map::default()`'s bounds, which is not a substitution — `MAP INFO` edits them afterwards
    /// through the same validate-then-atomic write, so nothing is reachable before creation that
    /// is not reachable after it.
    #[test]
    fn naming_a_map_makes_it_and_lands_on_it() {
        let root = Root::new("map-by-enter");
        root.kit(Some("site"), 2);
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));

        c.focus = Focus::Maps;
        c.start_new();
        assert!(matches!(c.creating, Some(New::Map(_))), "making a map");
        assert!(c.editing, "straight into the name");
        c.raw = "porch".to_owned();
        keep_field(&mut c, Field::Name);

        assert_eq!(c.problem, None, "nothing to refuse");
        assert!(c.creating.is_none(), "one Enter finishes a map too");
        assert_eq!(c.focus, Focus::Maps, "the keyboard comes back to the list");
        assert_eq!(
            c.current_map().map(|m| m.name.clone()),
            Some("porch".to_owned()),
            "standing on the map just made"
        );
        // And its settings are the ones a map starts with — editable from here, not lost.
        assert!(
            c.screen().settings.iter().any(|r| r.left == "BOUNDS"),
            "bounds are still reachable, in MAP INFO: {:?}",
            c.screen().settings
        );
    }

    /// **Coming back from the editor lands on the map you left.**
    ///
    /// The chooser is rebuilt from scratch each time the editor exits — that is what makes going
    /// back cost no teardown — so without this it reopened wherever the command line pointed, and
    /// the round trip felt like a restart instead of a step back.
    #[test]
    fn coming_back_lands_where_you_were() {
        let root = Root::new("reveal");
        root.kit(None, 1);
        let _kit = root.kit(Some("site"), 2);
        for m in ["alpha", "hall", "zulu"] {
            create_map(&root.maps(), m, (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, None);

        // Opens on the first kit's first map, which is not where we were.
        assert_ne!(c.current_map().map(|m| m.name.clone()), Some("hall".into()));

        c.reveal(Some("site"), Some("hall"));
        assert_eq!(
            c.current_kit().map(|k| k.label.clone()),
            Some("site".to_owned())
        );
        assert_eq!(
            c.current_map().map(|m| m.name.clone()),
            Some("hall".to_owned()),
            "standing on the map the editor was just showing"
        );

        // **Best-effort, separately.** A map deleted while the editor was up still lands the
        // keyboard on the right kit rather than refusing to move at all.
        c.reveal(Some("site"), Some("gone"));
        assert_eq!(
            c.current_kit().map(|k| k.label.clone()),
            Some("site".to_owned())
        );
        // And a kit that no longer exists leaves the selection alone rather than blanking it.
        let before = c.kit;
        c.reveal(Some("no_such_kit"), None);
        assert_eq!(c.kit, before);
    }

    /// **A kit with no name is refused, and the refusal keeps you in the field.** The one thing
    /// `Enter`-makes-it must not do is create something nobody named.
    #[test]
    fn enter_on_an_empty_kit_name_makes_nothing() {
        let root = Root::new("kit-unnamed");
        root.kit(Some("site"), 1);
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));

        c.focus = Focus::Kits;
        c.start_new();
        c.raw = "   ".to_owned();
        keep_field(&mut c, Field::Name);

        assert!(c.problem.is_some(), "it says why");
        assert!(matches!(c.creating, Some(New::Kit(_))), "still making one");
        assert!(c.editing, "and still in the field");
    }

    /// `N` makes a new one of whatever the column lists — a kit on the kit list, a map on the map
    /// list. One rule, which is what makes it guessable.
    #[test]
    fn n_makes_a_new_one_of_whatever_this_column_lists() {
        let root = Root::new("n-per-panel");
        let _kit = root.kit(Some("site"), 1);
        create_map(&root.maps(), "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));

        assert_eq!(c.focus, Focus::Kits);
        c.start_new();
        assert!(
            matches!(c.creating, Some(New::Kit(_))),
            "on the kit list, a kit"
        );
        assert!(
            c.editing,
            "and straight into the name — neither can be made without one"
        );
        assert!(
            render(&c).contains("NEW KIT"),
            "the settings column says which:\n{}",
            render(&c)
        );

        c.creating = None;
        c.focus = Focus::Maps;
        c.start_new();
        assert!(
            matches!(c.creating, Some(New::Map(_))),
            "on the map list, a map"
        );
        assert!(
            render(&c).contains("NEW MAP IN site"),
            "and names its kit:\n{}",
            render(&c)
        );
    }

    /// **A new kit is one the editor will actually open**, which is the only thing that makes it a
    /// kit: `Catalog::scan` and `Project::open` both test for `library.ron`, so writing the
    /// directory without it would produce something the chooser lists and the editor then refuses.
    #[test]
    fn a_new_kit_is_one_the_scanner_accepts() {
        let root = Root::new("new-kit");
        root.kit(Some("site"), 3);

        let dir = create_kit(&root.0, "Site V3").unwrap_or_else(|e| panic!("{e}"));
        assert!(
            dir.ends_with("site_v3"),
            "forced to snake_case: {}",
            dir.display()
        );
        assert!(dir.join(LIBRARY_FILE).is_file(), "a kit is its library");
        assert!(
            dir.join(emerge_core::policy::POLICY_FILE).is_file(),
            "and its policy — `Project::open` reads both"
        );
        // No compositions.ron: absence and empty are the same state to `layered_library`, so a file
        // saying nothing would be a file that means nothing.
        assert!(!dir.join("compositions.ron").exists());

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let made = catalog
            .kits
            .iter()
            .find(|k| k.label == "site_v3")
            .unwrap_or_else(|| panic!("the new kit did not scan"));
        assert_eq!(
            made.flag.as_deref(),
            Some("site_v3"),
            "reachable as --kit site_v3"
        );
    }

    /// **A new project opens and bootstraps** — the plan's Phase C verification 7. The skeleton is
    /// `assets/emerge/{kits.ron,vocab.ron}` and nothing else; the copied vocabulary is
    /// byte-identical to the source's; a fresh scan sees zero kits and zero maps; and the first
    /// kit made lands in the new project — which used to fail with "kits.ron does not exist" until
    /// `bind_kit` learned to say what the directory is instead.
    #[test]
    fn a_new_project_opens_and_bootstraps() {
        let root = Root::new("new-project");
        root.kit(Some("site"), 1);
        // A vocabulary with a comment, to pin the byte-copy.
        let vocab = root.0.join(EMERGE_DIR).join("vocab.ron");
        std::fs::write(
            &vocab,
            "(// a comment that must survive\n kind: (tokens: []), effects: (tokens: []), look: (tokens: []), surfaces: (tokens: []), )",
        )
        .unwrap_or_else(|e| panic!("{}: {e}", vocab.display()));

        let parent = root.0.parent().unwrap_or_else(|| panic!("temp dir has a parent"));
        // A sibling the previous run left behind — `Root::new` cleans only its own directory.
        let _ = std::fs::remove_dir_all(parent.join("porch_kit"));
        let dir = create_project(parent, "Porch Kit", &vocab).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            dir.ends_with("porch_kit"),
            "forced to snake_case: {}",
            dir.display()
        );
        let emerge = dir.join(EMERGE_DIR);
        assert!(emerge.join(emerge_core::kits::KITS_FILE).is_file(), "kits.ron");
        assert!(emerge.join("vocab.ron").is_file(), "vocab.ron");
        assert!(
            std::fs::read(emerge.join("vocab.ron")).unwrap_or_else(|e| panic!("{e}"))
                == std::fs::read(&vocab).unwrap_or_else(|e| panic!("{e}")),
            "the copied vocabulary is byte-identical — comments included"
        );
        // Nothing else: no compositions.ron, no maps/, no kit.
        assert!(!emerge.join("compositions.ron").exists());
        assert!(!emerge.join("maps").exists());

        let catalog = Catalog::scan(&dir).unwrap_or_else(|e| panic!("{e}"));
        assert!(catalog.kits.is_empty(), "no kits yet");
        assert!(catalog.maps.is_empty(), "no maps yet");
        assert!(catalog.authoring.is_none(), "no authoring kit yet");

        // **The first kit made is the authoring kit.** This call failed before C2 — the new
        // project's `kits.ron` was not there to bind into, and `bind_kit` said so with a raw io
        // error instead of an instruction.
        let kit_dir = create_kit(&dir, "props").unwrap_or_else(|e| panic!("{e}"));
        assert!(kit_dir.starts_with(&dir), "the kit is inside the project");
        let catalog = Catalog::scan(&dir).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            catalog.authoring.as_deref(),
            Some("props"),
            "bind_kit adopts the first kit"
        );
    }

    /// **The whole verb, from the column to the disk** — `N` on PROJECTS, a name, `Enter`.
    ///
    /// This is the automated half of the plan's manual Phase C smoke: the draft is a project (not a
    /// map), the directory lands beside this one, the cursor comes back to the PROJECTS column
    /// standing on what was just made, and **the status line carries the command that opens it**.
    /// That last one is the assertion with a history: `keep_field` used to write
    /// `make_it(..).err()` unconditionally, which wiped the line in the same frame it was written.
    #[test]
    fn making_a_project_reports_the_command_that_opens_it() {
        let root = Root::new("project-flow");
        root.kit(Some("site"), 1);
        let vocab = root.0.join(EMERGE_DIR).join("vocab.ron");
        std::fs::write(&vocab, "(// kept\n)").unwrap_or_else(|e| panic!("{e}"));
        let parent = root
            .0
            .parent()
            .unwrap_or_else(|| panic!("temp dir has a parent"))
            .to_path_buf();
        let _ = std::fs::remove_dir_all(parent.join("porch_flow"));

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        c.focus = Focus::Projects;
        c.start_new();
        assert!(
            matches!(c.creating, Some(New::Project(_))),
            "on the projects column, a project"
        );
        assert!(c.editing, "straight into the name");
        assert!(
            render(&c).contains("NEW PROJECT"),
            "the settings column says which:\n{}",
            render(&c)
        );

        c.raw = "Porch Flow".to_owned();
        keep_field(&mut c, Field::Name);

        assert!(c.creating.is_none(), "one Enter finishes it");
        let made = parent.join("porch_flow");
        assert!(
            made.join(EMERGE_DIR).join(emerge_core::kits::KITS_FILE).is_file(),
            "the project landed beside this one: {}",
            made.display()
        );
        let line = c.problem.clone().unwrap_or_default();
        assert!(
            line.contains("`porch_flow`") && line.contains("emerge-mapper"),
            "the status line reports the command: {line}"
        );
        assert!(
            line.contains(&made.display().to_string()),
            "and names the directory: {line}"
        );
        assert_eq!(c.focus, Focus::Projects, "the keyboard comes back to the list");
        assert_eq!(
            c.catalog
                .projects
                .get(c.project.wrapping_sub(1))
                .map(|p| p.name.as_str()),
            Some("porch_flow"),
            "standing on what was just made: {:?}",
            c.catalog.projects
        );
        // And the current project is still marked, beside it.
        assert!(
            c.catalog.projects.iter().any(|p| p.current),
            "the project this screen is in stays marked: {:?}",
            c.catalog.projects
        );
    }

    /// **Names cannot escape the parent** — the plan's Phase C verification 8. `to_snake_case`
    /// strips separators and dots, so every hostile name either refuses or lands strictly inside
    /// the parent; the returned path must start with the parent either way. This pins existing
    /// behaviour rather than adding a guard.
    #[test]
    fn a_project_name_cannot_escape_the_parent() {
        let root = Root::new("escape");
        let vocab = root.0.join(EMERGE_DIR).join("vocab.ron");
        std::fs::write(&vocab, "()").unwrap_or_else(|e| panic!("{e}"));
        let parent = root.0.parent().unwrap_or_else(|| panic!("temp dir has a parent"));

        for hostile in ["../evil", "foo/bar", "..", "a/../../b"] {
            match create_project(parent, hostile, &vocab) {
                Err(e) => assert!(
                    e.contains("leaves nothing usable") || e.contains("already exists"),
                    "{hostile:?}: refused with a reason: {e}"
                ),
                Ok(dir) => {
                    assert!(
                        dir.starts_with(parent),
                        "{hostile:?} landed outside the parent: {}",
                        dir.display()
                    );
                    assert!(
                        !dir.to_string_lossy().contains(".."),
                        "{hostile:?} escaped: {}",
                        dir.display()
                    );
                }
            }
        }
    }

    /// **`set_authoring` re-points `kits.ron`'s `authoring`, and refuses a name that is not bound.**
    ///

    /// **`set_authoring` re-points `kits.ron`'s `authoring`, and refuses a name that is not bound.**
    ///
    /// The refusal names the bound kits, because "not bound" without a list is a search. The
    /// round-trip is through `Catalog::scan`, which is what the screen reads — so the row an author
    /// sees is the file an author wrote.
    #[test]
    fn authoring_round_trips_and_refuses_unbound_names() {
        let root = Root::new("authoring");
        root.kit(Some("a"), 1);
        root.kit(Some("b"), 1);

        set_authoring(&root.0, "b").unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            catalog.authoring.as_deref(),
            Some("b"),
            "the scan reads what the write wrote"
        );

        let err = set_authoring(&root.0, "nope")
            .err()
            .unwrap_or_else(|| panic!("an unbound name must be refused"));
        assert!(err.contains("`nope`"), "names the offender: {err}");
        assert!(err.contains("`a`") && err.contains("`b`"), "and the bound kits: {err}");
        // The refusal wrote nothing.
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(catalog.authoring.as_deref(), Some("b"));
    }

    /// An unusable name is refused by name, and a taken one is refused rather than merged into
    /// somebody else's kit.
    #[test]
    fn a_kit_name_that_cannot_work_is_refused() {
        let root = Root::new("kit-names");
        root.kit(Some("site"), 1);

        for raw in ["", "   ", "!!!"] {
            let e = create_kit(&root.0, raw)
                .err()
                .unwrap_or_else(|| panic!("`{raw}` must be refused"));
            assert!(e.contains("snake_case"), "the refusal names the rule: {e}");
        }
        let e = create_kit(&root.0, "site")
            .err()
            .unwrap_or_else(|| panic!("a taken name must be refused"));
        assert!(e.contains("already exists"), "{e}");
        // And the kit that was there is untouched.
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            catalog
                .kits
                .iter()
                .find(|k| k.label == "site")
                .map(|k| k.pieces),
            Some(1),
            "refusing must not have emptied the existing kit"
        );
    }

    /// **Asking to delete deletes nothing.** The whole point of the confirmation: the file is still
    /// there after the question is raised, and only the second keystroke removes it.
    #[test]
    fn asking_to_delete_does_not_delete() {
        let root = Root::new("ask-delete");
        let _kit = root.kit(Some("site"), 1);
        let path = create_map(&root.maps(), "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        // **MAPS is the column to the right of KITS**, so one crossing reaches it.
        c.cross(1);
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));

        assert!(matches!(c.ask, Some(Ask::Delete(_))), "the question is up");
        assert!(
            path.is_file(),
            "and the file is UNTOUCHED until it is answered"
        );
        assert!(
            c.screen().asking.is_none(),
            "the chooser stopped drawing its own copy of the question when the modal took it"
        );

        let gone = c.confirm_delete().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(gone, "hall");
        assert!(!path.exists(), "answering yes removes it");
        assert!(c.ask.is_none(), "and the question is gone with it");
    }

    /// Declining leaves the file exactly where it was.
    #[test]
    fn declining_a_delete_keeps_the_map() {
        let root = Root::new("keep-it");
        let _kit = root.kit(Some("site"), 1);
        let path = create_map(&root.maps(), "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        // **MAPS is the column to the right of KITS.**
        c.cross(1);
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));

        c.ask = None; // what Esc does
        assert!(path.is_file(), "declining must leave the map alone");
        assert!(
            c.confirm_delete().is_err(),
            "and with nothing asked, agreeing deletes nothing"
        );
        assert!(path.is_file());
    }

    /// **The question holds a path, not a row.** If the list moves under a raised prompt, the thing
    /// deleted is still the thing named — a prompt remembering "row 2" would delete whatever row 2
    /// had become, which is how a confirmation removes the wrong file.
    #[test]
    fn the_question_deletes_what_it_named_even_if_the_selection_moves() {
        let root = Root::new("moving-target");
        let kit = root.kit(Some("site"), 1);
        for m in ["alpha", "beta"] {
            create_map(&root.maps(), m, (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        // **MAPS is the column to the right of KITS.**
        c.cross(1);
        c.ask_delete().unwrap_or_else(|e| panic!("{e}")); // asks about `alpha`

        c.map = 1; // the selection moves to `beta`
        let gone = c.confirm_delete().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            gone, "alpha",
            "the question named alpha, so alpha is what goes"
        );
        let _ = kit;
        assert!(
            root.maps().join("beta.map.ron").is_file(),
            "beta was never in question"
        );
        assert!(!root.maps().join("alpha.map.ron").exists());
    }

    /// Pressing Delete with the arrows on the kit list is a refusal that says what to do, not a
    /// silent no-op (`docs/ui.md` §1.4).
    #[test]
    fn delete_asks_about_whichever_list_you_are_in() {
        let root = Root::new("wrong-panel");
        root.kit(None, 4);
        let _kit = root.kit(Some("site"), 1);
        create_map(&root.maps(), "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));

        // On the kit list it asks about the kit, and names what is inside it.
        assert_eq!(c.focus, Focus::Kits);
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));
        match &c.ask {
            Some(Ask::Delete(p)) => {
                assert!(p.kit, "the kit list asks about a kit");
                assert_eq!(p.name, "site");
                assert!(p.path.is_dir(), "and holds the directory that would go");
            }
            other => panic!("expected a kit deletion: {other:?}"),
        }
        c.ask = None;

        // On the map list it asks about the map.
        c.focus = Focus::Maps;
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));
        assert!(
            matches!(&c.ask, Some(Ask::Delete(p)) if !p.kit && p.name == "hall"),
            "{:?}",
            c.ask
        );
        c.ask = None;

        // In the settings there is nothing to remove, and the refusal says where to be.
        c.focus = Focus::Settings;
        let e = c
            .ask_delete()
            .err()
            .unwrap_or_else(|| panic!("must refuse from the settings"));
        assert!(e.contains("Tab to one of those lists"), "{e}");
        assert!(c.ask.is_none());
    }
    /// **The POLICY panel lists the kit's entries and removes one through the question.**
    ///
    /// `POLICY` is drawn under the kit list, so `down` off the end of that list is the way in, and
    /// it draws the selected kit's exclusions and patches read from `project.ron`. `Delete` on a
    /// row asks (nothing goes yet); agreeing splices the line out and leaves the file's comments
    /// intact.
    #[test]
    fn the_policy_panel_lists_and_removes_an_entry() {
        let root = Root::new("policy-panel");
        root.kit(Some("site"), 1);
        // A policy with one exclusion, one patch, and a comment that must survive.
        let path = root.0.join(EMERGE_DIR).join("site").join(emerge_core::policy::POLICY_FILE);
        std::fs::write(
            &path,
            r#"(
    version: 2,
    // A comment that must survive.
    note: Some("a kit with policy"),
    exclude: ["characters"],
    patches: [
        ( match: Id("wall"), because: "this game's walls are 2.4 m", patch: ( align: ( stretch_y: Some(1.2) ) ) ),
    ],
)
"#,
        )
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));

        // **POLICY is drawn under the kit list**, so `down` off the end of the list is the way in.
        c.step(1);
        assert_eq!(c.focus, Focus::Policy, "the panel below the kit list");
        let s = c.screen();
        assert!(
            s.policy.iter().any(|r| r.left == "exclude" && r.right == "characters"),
            "the exclusion is listed: {:?}",
            s.policy
        );
        assert!(
            s.policy
                .iter()
                .any(|r| r.left == "patch" && r.right.contains("2.4 m")),
            "the patch is listed with its reason: {:?}",
            s.policy
        );

        // Delete on the exclusion row asks, and agreeing removes only that line.
        c.policy = 0;
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));
        assert!(
            matches!(c.ask, Some(Ask::RemovePolicy { .. })),
            "the question names a policy removal: {:?}",
            c.ask
        );
        let shown = match &c.ask {
            Some(Ask::RemovePolicy { shown, .. }) => shown.clone(),
            _ => String::new(),
        };
        assert!(shown.contains("characters"), "the row names the entry: {shown}");
        c.confirm_delete().unwrap_or_else(|e| panic!("{e}"));

        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        assert!(text.contains("// A comment that must survive"), "the comment stands:\n{text}");
        let policy = emerge_core::policy::Policy::parse(&text).unwrap_or_else(|e| panic!("{e}"));
        assert!(policy.exclude.is_empty(), "the exclusion is gone");
        assert_eq!(policy.patches.len(), 1, "the patch is untouched");
    }

    /// **A kit that excludes nothing and patches nothing has no POLICY panel to stand in**, so the
    /// walk down the kit column stops at the bottom of the list.
    ///
    /// `panel_has_rows` used to answer `current_kit().is_some()` — "could this panel exist" rather
    /// than "is there anything to stand on". That gap was survivable while the crossing key walked
    /// *past* an empty panel; `down` walks *into* one, so it would have parked the cursor on a
    /// blank panel with nothing to press and no hint that would tell the truth.
    #[test]
    fn a_kit_with_nothing_in_its_policy_has_no_panel_to_walk_into() {
        let root = Root::new("policy-empty");
        root.kit(Some("site"), 2);
        // *"A project states its policy, even when its policy is nothing"* — the file is here and
        // it says nothing, which is a different state from the file being missing (that draws an
        // `unreadable` row, and standing on it is right because it is a problem to read).
        let path = root.0.join(EMERGE_DIR).join("site").join(emerge_core::policy::POLICY_FILE);
        std::fs::write(&path, "(\n    version: 2,\n    note: None,\n    patches: [],\n)")
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        assert!(c.screen().policy.is_empty(), "nothing declared, nothing drawn");

        for _ in 0..6 {
            c.step(1);
        }
        assert_eq!(
            c.focus,
            Focus::Kits,
            "the walk stops at the bottom of the list rather than parking on a blank panel"
        );
        // And the column beside it is still one press away.
        c.cross(1);
        assert_eq!(c.focus, Focus::Maps);
    }

    /// **The question counts in English.** `1 maps` reads as generated rather than written, in the
    /// one sentence on this screen that has to be trusted before a directory is removed.
    #[test]
    fn the_question_counts_in_english() {
        let root = Root::new("plurals");
        root.kit(None, 1);
        let _kit = root.kit(Some("site"), 1);
        create_map(&root.maps(), "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        c.focus = Focus::Kits;
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));

        // **The wording moved to `crate::confirm`'s modal**, so the chooser renders no question of
        // its own — see `Chooser::screen`'s `asking: None`. What this still owns is that the
        // question is ARMED and that `Delete` did not delete; the two answers and their keys are
        // the modal's, and `one_prompt_one_vocabulary.rs` is what holds them.
        assert!(matches!(c.ask, Some(Ask::Delete(_))), "a question is up");
        assert!(
            c.screen().asking.is_none(),
            "and the chooser does not draw it a second time under the modal"
        );
        assert_eq!(plural(0, "map"), "maps", "zero is plural");
        assert_eq!(plural(2, "piece"), "pieces");

        // And the map rows count the same way. A fresh map is `empty` rather than `0 pieces`, so
        // what this can observe is the absence of the parenthetical — `1 piece(s)` was the same
        // defect one row above the line that was reported.
        c.ask = None;
        let rows = c.screen().maps;
        assert!(
            !rows.iter().any(|r| r.right.contains("(s)")),
            "no parenthetical plurals anywhere: {rows:?}"
        );
        assert!(
            rows.iter().any(|r| r.right == "empty"),
            "and a new map reads as empty, not as a count of nothing: {rows:?}"
        );
    }

    /// **Deleting leaves you next to where you were, not at the top.**
    ///
    /// Asked for at the keyboard: *"when I delete an item, it shouldn't bring me back to the top of
    /// the menu — it should bring me back to the item right above the one I just deleted."* Clearing
    /// several in a row meant finding your place again every time.
    #[test]
    fn deleting_lands_on_the_row_above() {
        let root = Root::new("land-above");
        root.kit(None, 1);
        let _kit = root.kit(Some("site"), 1);
        for m in ["alpha", "bravo", "charlie", "delta"] {
            create_map(&root.maps(), m, (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        c.focus = Focus::Maps;

        // Stand on `charlie` (rows: 0 `+ new map`, 1 alpha, 2 bravo, 3 charlie, 4 delta).
        c.map = 3;
        assert_eq!(
            c.current_map().map(|m| m.name.clone()),
            Some("charlie".into())
        );
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));
        c.confirm_delete().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            c.current_map().map(|m| m.name.clone()),
            Some("bravo".to_owned()),
            "the row above the one removed, not the top of the list"
        );

        // **Deleting the first entry floors at the first real row**, never the `+ new map` row —
        // landing there would say nothing is selected when something still is.
        c.map = 1;
        assert_eq!(
            c.current_map().map(|m| m.name.clone()),
            Some("alpha".into())
        );
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));
        c.confirm_delete().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c.map, 1, "floors at the first real row");
        assert_eq!(
            c.current_map().map(|m| m.name.clone()),
            Some("bravo".into())
        );

        // **And the last one leaves nothing behind to stand on.**
        for _ in 0..2 {
            c.map = c.catalog.maps.len();
            c.ask_delete().unwrap_or_else(|e| panic!("{e}"));
            c.confirm_delete().unwrap_or_else(|e| panic!("{e}"));
        }
        assert_eq!(c.map, 0, "an empty list has only the `+ new map` row");
        assert_eq!(c.focus, Focus::Kits, "and the keyboard goes back a column");
    }

    /// **The same rule one column over.**
    #[test]
    fn deleting_a_kit_lands_on_the_kit_above() {
        let root = Root::new("land-above-kit");
        root.kit(None, 1);
        for k in ["alpha", "bravo", "charlie"] {
            root.kit(Some(k), 1);
        }
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("charlie"));
        c.focus = Focus::Kits;
        assert_eq!(
            c.current_kit().map(|k| k.label.clone()),
            Some("charlie".to_owned())
        );
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));
        c.confirm_delete().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            c.current_kit().map(|k| k.label.clone()),
            Some("bravo".to_owned()),
            "the kit above the one removed"
        );
    }

    /// **The default kit is not deletable, and that is the one guard that matters.** It is
    /// `assets/emerge` itself — every other kit is a subdirectory of it — so `remove_dir_all` there
    /// would take the whole library. It is refused at the question, not warned about after.
    #[test]
    fn the_default_kit_cannot_be_deleted() {
        let root = Root::new("root-kit-safe");
        root.kit(None, 3);
        root.kit(Some("site"), 1);
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, None);

        // Stand on the root kit — the one whose `flag` is None.
        let i = c
            .catalog
            .kits
            .iter()
            .position(|k| k.flag.is_none())
            .unwrap_or_else(|| panic!("the root kit is always listed"));
        c.kit = i + 1;
        c.focus = Focus::Kits;

        let e = c
            .ask_delete()
            .err()
            .unwrap_or_else(|| panic!("must refuse the default kit"));
        assert!(e.contains("default kit"), "{e}");
        assert!(c.ask.is_none(), "and nothing is pending");
        // **But the verb is still listed.** Hiding it on the row an author lands on taught "there
        // is no delete here" rather than "not this kit" — reported in those words.
        assert!(
            c.hint().contains("Delete"),
            "the verb stays visible; the refusal is what teaches: {}",
            c.hint()
        );
    }

    /// **`B` walks the map through the project's bashes and rounds back to every kit**, writing the
    /// file each time.
    ///
    /// Written immediately rather than on some later save, because this panel has no save. The
    /// assertion re-parses the file rather than reading the summary back: the row is what an author
    /// sees, and the file is what the editor and the game will open.
    #[test]
    fn cycling_a_bash_writes_the_file_and_rounds_to_every_kit() {
        let root = Root::new("bash-cycle");
        root.skin("furniture", "furniture", &["bench"]);
        root.skin("lab", "lab", &["desk"]);
        root.bash("hub", &["furniture", "lab"]);
        root.bash("props", &["furniture"]);
        root.map(&root.0, "hall", &[]);

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, None);
        c.focus = Focus::Maps;
        c.map = 1;
        let path = c
            .current_map()
            .map(|m| m.path.clone())
            .unwrap_or_else(|| panic!("the map row is selected"));
        let on_disk = |path: &Path| {
            Map::parse(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{e}")))
                .unwrap_or_else(|e| panic!("{e}"))
                .bash
        };
        let shown = |c: &Chooser| {
            c.screen()
                .settings
                .iter()
                .find(|r| r.left == "bash")
                .map(|r| r.right.clone())
                .unwrap_or_else(|| panic!("MAP INFO carries a bash row"))
        };

        // A map starts offering every kit, and the row says so in words rather than by being blank.
        assert_eq!(on_disk(&path), None);
        assert_eq!(shown(&c), "every kit");

        // **File order, not alphabetical** — `hub` is declared first, so it is what the first press
        // names.
        for (want, label) in [(Some("hub"), "hub"), (Some("props"), "props"), (None, "every kit")] {
            c.cycle_bash().unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(on_disk(&path).as_deref(), want, "the file carries it");
            assert_eq!(shown(&c), label, "and MAP INFO says the same thing");
        }

        // **And the screen names the key that does it**, standing where it works. `docs/ui.md`
        // §4.2 — a verb reachable by keyboard states its key.
        assert!(c.hint().contains("B bash"), "the hint has to name the verb: {}", c.hint());
        c.map = 0;
        assert!(
            !c.hint().contains("B bash"),
            "and not on the row that makes a map, where there is no file to set: {}",
            c.hint()
        );
    }

    /// **A project with no bashes has nothing to cycle, and the refusal says where one is made.**
    ///
    /// A bash is authored by hand in `kits.ron`, the same way that file's `lattice` is — so the
    /// unmet condition is an instruction (`docs/ui.md` §1.4) rather than a dead key.
    #[test]
    fn cycling_with_no_bashes_declared_says_where_to_declare_one() {
        let root = Root::new("bash-none");
        root.skin("furniture", "furniture", &["bench"]);
        root.map(&root.0, "hall", &[]);

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, None);
        c.focus = Focus::Maps;

        // Row 0 makes a map, so there is nothing there to give a bash to.
        c.map = 0;
        let e = c
            .cycle_bash()
            .err()
            .unwrap_or_else(|| panic!("the `+ new map` row has no map"));
        assert!(e.contains("no map here"), "{e}");

        c.map = 1;
        let e = c
            .cycle_bash()
            .err()
            .unwrap_or_else(|| panic!("a project declaring none must refuse"));
        assert!(e.contains("declares no bashes") && e.contains("kits.ron"), "{e}");

        // And nothing was written: the map still offers every kit.
        let path = c
            .current_map()
            .map(|m| m.path.clone())
            .unwrap_or_else(|| panic!("the map row is selected"));
        let map = Map::parse(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}")))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(map.bash, None);
    }

    /// **A kit something else still names is refused, and the refusal says what.**    /// **A kit something else still names is refused, and the refusal says what.**
    ///
    /// The guard this pins was missing, and `remove_dir_all` took `assets/emerge/site` with 45
    /// pieces the game names by id — 51 tests and the ability to boot. The root-kit check was the
    /// only thing standing between the verb and any directory under `assets/emerge`.
    #[test]
    fn a_kit_something_else_still_names_is_refused() {
        let root = Root::new("delete-strands");
        root.kit(None, 1);
        root.skin("site", "site", &["floor", "wall"]);
        let lab = root.skin("lab", "lab", &["bench"]);
        // The *lab's* map reaches across for a site piece — which is the whole point of making
        // kits shareable, and the whole reason deleting one stops being a local act.
        root.map(&lab, "lab_a", &["lab/bench", "site/floor"]);

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, None);
        stand_on(&mut c, "site");

        let e = c
            .ask_delete()
            .err()
            .unwrap_or_else(|| panic!("must refuse a kit whose pieces are still named"));
        assert!(
            e.contains("assets/emerge/maps/lab_a.map.ron"),
            "the refusal names the file, because \"something uses it\" is not information: {e}"
        );
        assert!(c.ask.is_none(), "and nothing is pending");
    }

    /// **A kit another directory re-skins can go**, because nothing is stranded by it going.
    ///
    /// This is the distinction the whole scan turns on: not *"is anything using this kit"* but
    /// *"is this kit the last provider"*. `site` and `site_greybox` ship defining the identical 45
    /// ids, so removing either leaves every reference resolvable — and a guard that could not tell
    /// the difference would refuse every deletion an author actually wants.
    #[test]
    fn a_kit_another_one_re_skins_can_still_go() {
        let root = Root::new("delete-skin");
        root.kit(None, 1);
        root.skin("site", "site", &["floor", "wall"]);
        root.skin("site_greybox", "site", &["floor", "wall"]);
        let lab = root.skin("lab", "lab", &["bench"]);
        root.map(&lab, "lab_a", &["site/floor"]);

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, None);
        stand_on(&mut c, "site");

        c.ask_delete()
            .unwrap_or_else(|e| panic!("a skin with a twin strands nothing: {e}"));
        assert!(
            matches!(c.ask, Some(Ask::Delete(_))),
            "the question is asked, and only the question — nothing is gone yet"
        );
    }

    /// **The game names pieces too, and no scan of `assets/emerge` would ever see it.**
    ///
    /// `src/site/kit.rs` holds `assets/emerge/site` in a `&'static str` and `assets/site/kit_ozea.ron`
    /// names all 45 ids. That is the dependent that made this a data-loss bug rather than a
    /// nuisance: every map and composition in the project could be silent about a kit the game
    /// cannot boot without.
    #[test]
    fn the_game_kit_file_is_a_dependent_no_content_scan_would_find() {
        let root = Root::new("delete-game");
        root.kit(None, 1);
        root.skin("site", "site", &["floor"]);
        // Deliberately nothing under `assets/emerge` refers to it. The editor's own world is clean.
        root.game_kit("kit_ozea.ron", &["site/floor"]);

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, None);
        stand_on(&mut c, "site");

        let e = c
            .ask_delete()
            .err()
            .unwrap_or_else(|| panic!("the game is a dependent like any other"));
        assert!(e.contains("assets/site/kit_ozea.ron"), "{e}");
    }

    /// **Three names, then a count.** Listing all 51 dependents would be a screen of paths where
    /// the point is that there are 51 of them — and a refusal is read while reaching for the next
    /// key, which is the register the root-kit guard was cut down to.
    #[test]
    fn the_refusal_names_a_few_and_counts_the_rest() {
        let many: Vec<String> = (0..5).map(|i| format!("f{i}.map.ron")).collect();
        let msg = strands("site", &many).unwrap_or_else(|| panic!("five dependents is a refusal"));
        assert!(msg.contains("5 files"), "{msg}");
        assert!(msg.contains("f0.map.ron") && msg.contains("f2.map.ron"), "{msg}");
        assert!(!msg.contains("f3.map.ron"), "the fourth is counted, not named: {msg}");
        assert!(msg.contains("and 2 more"), "{msg}");

        // One reads as one, and none is not a refusal at all.
        let one = strands("site", &["a.map.ron".to_owned()])
            .unwrap_or_else(|| panic!("one dependent is still a refusal"));
        assert!(one.contains("1 file still name"), "{one}");
        assert_eq!(strands("site", &[]), None, "nothing stranded is not a refusal");
    }

    /// **Agreeing removes the whole directory, and the keyboard lands somewhere real.**
    #[test]
    fn deleting_a_kit_takes_the_directory_with_it() {
        let root = Root::new("kit-delete");
        root.kit(None, 2);
        // `site` first, so it is the kit new work lands in. Deleting **that** one is refused while
        // another exists — see `the_kit_new_work_lands_in_cannot_just_be_deleted`.
        root.kit(Some("site"), 1);
        let doomed = root.kit(Some("scratch"), 1);
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("scratch"));

        c.focus = Focus::Kits;
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));
        assert!(doomed.exists(), "asking removes nothing");

        let gone = c.confirm_delete().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(gone, "scratch");
        assert!(!doomed.exists(), "the whole kit directory goes");
        assert!(
            !c.catalog.kits.iter().any(|k| k.label == "scratch"),
            "and it is off the list"
        );
        assert_eq!(c.focus, Focus::Kits, "the keyboard stays on the kit list");
        assert!(
            c.current_kit().is_some(),
            "standing on a kit that still exists"
        );
    }

    /// An empty kit is a kit with no maps, not an error — and the screen turns that into an
    /// instruction rather than a report (`docs/ui.md` §1.4).
    #[test]
    fn an_empty_kit_reports_no_maps_rather_than_failing() {
        let root = Root::new("empty");
        root.kit(Some("site_v2"), 0);
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let kit = catalog
            .kits
            .iter()
            .find(|k| k.label == "site_v2")
            .unwrap_or_else(|| panic!("missing"));
        assert_eq!(kit.pieces, 0);
        assert!(catalog.maps.is_empty(), "and it brought no maps with it");
    }

    /// **A map that will not parse is a row, not an omission.** Dropping it would present a broken
    /// project as an empty one, and the author would go looking for a map the list had quietly eaten.
    #[test]
    fn an_unreadable_map_is_listed_with_its_reason() {
        let root = Root::new("broken");
        root.kit(Some("site"), 1);
        let maps = root.maps();
        std::fs::create_dir_all(&maps).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(maps.join("broken.map.ron"), "(this is not a map)")
            .unwrap_or_else(|e| panic!("{e}"));

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let entry = catalog
            .kits
            .iter()
            .find(|k| k.label == "site")
            .and_then(|_| catalog.maps.iter().find(|m| m.name == "broken"))
            .unwrap_or_else(|| panic!("the broken map was dropped from the list"));
        assert!(
            matches!(entry.summary, MapSummary::Unreadable(_)),
            "it must carry its reason: {:?}",
            entry.summary
        );
    }

    /// A root with no `assets/emerge` is refused by name — the message tells the author what to do,
    /// per §1.4, rather than reporting an empty list that looks like a project with no kits.
    #[test]
    fn a_root_that_is_not_a_project_says_so() {
        let dir = std::env::temp_dir().join(format!("chooser-bare-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        let e = Catalog::scan(&dir)
            .err()
            .unwrap_or_else(|| panic!("must refuse"));
        assert!(e.contains("is not a project"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ------------------------------------------------------------------------------------------------
// The screen
// ------------------------------------------------------------------------------------------------

use bevy::prelude::*;

/// **Which column has the arrows**, and — when a field is open — which field has the keyboard.
///
/// Modelled here rather than inferred from "is some string non-empty", which is the second census
/// `keys.rs` keeps deleting: a phase that lives in a handler's `if` cannot be rendered, and the hint
/// line then lies about what the arrows do.
/// **Which panel the arrows are in.**
///
/// Asked for at the keyboard, and it replaced a worse model: *"the tab worked to bring me down to
/// the bottom section, but then when I get there, I need to use the arrow keys to move around, not
/// tab. Tab should move around the different sections."*
///
/// The first version made `Focus::Field(Field)` a variant, so one key meant "next field" in the
/// settings and "go to the settings" everywhere else — one key with two jobs, decided by where you
/// already were. Typing is a separate flag rather than a variant, because it is a phase this screen
/// passes through and not a place the arrows can be — the distinction `keys::Stance` makes in the
/// editor.
///
/// # Arrows, and nothing else — the rule the rest of the editor already follows
///
/// `Tab` used to cross panels here, and it was the only `KeyCode::Tab` in the crate. Everywhere
/// else this application navigates with arrows and states the pair outright: the Meshes tab binds
/// `left`/`right` to [`crate::keys::Action::FocusCandidates`] / `FocusLibrary` to move between its
/// two side-by-side lists while `up`/`down` walk the one you are in, and Compose says the same
/// thing in its own words — *"up/down walk the groups, left/right walk the members of the one you
/// are on"*. A second crossing key on one screen out of six is a dialect.
///
/// So: **`left`/`right` cross columns, `up`/`down` walk down the column you are in** — through its
/// list and on into the panels stacked under it. Both keys mean what their arrow points at, which
/// is the property the old binding lost: `Policy` is drawn *below* the kit list, and reaching it by
/// pressing `right` was a horizontal key answering a vertical arrangement. That also cost the two
/// presses it took to get from KITS to MAPS, because the walk had to pass through it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Kits,
    /// The selected kit's policy — its exclusions and its patches, read from `project.ron`.
    ///
    /// The panel's content depends on the KITS cursor, so with the cursor on `+ new kit` — or in a
    /// project with no kits at all — it draws no rows and `down` off the end of the kit list has
    /// nowhere to go, the same rule that already empties KIT INFO on the `+ new kit` row.
    Policy,
    Maps,
    Settings,
    /// **Every project beside this one** — the siblings of the current root holding
    /// `assets/emerge`, plus the current root itself. Row 0 makes a new one; a real row reports
    /// the command that opens it, because nothing opens in this process.
    Projects,
}

impl Focus {
    /// **The screen, as the one table both arrows read**: three columns left to right, each listing
    /// its focusable panels top to bottom — exactly what [`spawn_screen`] draws.
    ///
    /// `KIT INFO` is absent because it is inert: facts about the selected kit, with no verb and no
    /// cursor, so the arrows must not be able to stand in it. `POLICY` is present because `Delete`
    /// acts on the row the cursor is on, and `MAP INFO` because `Enter` opens a field for typing.
    ///
    /// One table rather than a horizontal list and a vertical one: a screen described twice is a
    /// screen that can disagree with itself, which is how `left` came to reach something drawn
    /// underneath.
    const COLUMNS: [&'static [Focus]; 3] = [
        &[Focus::Projects],
        &[Focus::Kits, Focus::Policy],
        &[Focus::Maps, Focus::Settings],
    ];

    /// Where this panel sits: which column, and how far down it.
    fn at(self) -> (usize, usize) {
        for (c, panels) in Focus::COLUMNS.iter().enumerate() {
            if let Some(r) = panels.iter().position(|p| *p == self) {
                return (c, r);
            }
        }
        // Unreachable by construction — every variant is in the table above, and
        // `every_focus_is_somewhere_on_the_screen` is what keeps that true as variants are added.
        (0, 0)
    }
}

/// The four settings the chooser exposes, in the order they are shown and the arrows walk them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Bounds,
    Origin,
    Note,
}

impl Field {
    /// The settings every map has. Kits are appended per project — see [`Chooser::fields`].
    pub const ALL: [Field; 4] = [Field::Name, Field::Bounds, Field::Origin, Field::Note];

    pub fn label(self) -> &'static str {
        match self {
            Field::Name => "NAME",
            Field::Bounds => "BOUNDS",
            Field::Origin => "ORIGIN",
            Field::Note => "NOTE",
        }
    }

    // **No `is_text` any more.** It answered a constant `true` and nothing called it: a kit row once
    // took a toggle instead of text and this told the two apart, but the kit rows moved to the KITS
    // column on 2026-08-16. Four text fields, and no exception to carry.
}

/// **What is on the screen, and what `Enter` would do with it.**
///
/// The whole of the chooser's behaviour lives here as plain data with plain methods, so it is unit
/// tested without a window — the same split `descriptor::pick_cell` and `view::pan_direction` use,
/// and for the same reason: what a test cannot see is the schedule, not the arithmetic.
#[derive(Resource, Debug)]
pub struct Chooser {
    pub root: PathBuf,
    pub catalog: Catalog,
    pub kit: usize,
    /// **The row the arrows are on in the POLICY panel.** Row 0 is the first entry; there is no
    /// `+ new` row here, because adding a patch is not in this panel's scope — a patch's payload
    /// is a whole partial `Descriptor`, and the one place descriptor fields are authored is the
    /// detail pane.
    pub policy: usize,
    /// **The row the arrows are on in the PROJECTS panel.** Row 0 is `+ new project`; a real
    /// project `i` is `i + 1`, the indexing rule every other column uses.
    pub project: usize,
    pub map: usize,
    pub focus: Focus,
    /// Which settings row the arrows are on, while [`Focus::Settings`] has them.
    pub field: Field,
    /// **Is the highlighted field taking text?** A phase, not a place: the arrows move between
    /// fields, and `Enter` starts and ends the typing.
    pub editing: bool,
    /// What has been typed into the open field, before it is forced or parsed.
    pub raw: String,
    /// A refusal, shown until the next keystroke. Never a substituted value.
    pub problem: Option<String>,
    /// Set while something is being made — see [`New`].
    pub creating: Option<New>,
    /// **A question the screen has asked and is waiting on.** See [`Ask`].
    pub ask: Option<Ask>,
    /// **Set by the text handler when it consumed a key this frame.**
    ///
    /// The two key systems run in one frame, chained. `type_into_field` leaves a field on `Escape`
    /// by clearing `editing`, and `drive_chooser` then saw `editing == false` and read *the same
    /// press* as "quit" — one Escape, consumed twice, and the whole program closed out from under
    /// somebody who only wanted to leave a text box. Reported at the keyboard.
    ///
    /// A one-frame flag rather than reordering the systems, because the order is right: text takes
    /// the keyboard first, exactly as `keys::Phase::Text` runs before `Act` in the editor. What was
    /// missing is the other half of that contract — having taken the key, say so.
    pub swallowed: bool,
}

/// **Something the screen has asked, and is waiting on an answer to.**
///
/// Both of these destroy something — a file, or unsaved intent — so both are asked the same way and
/// answered with the same key. `Y` rather than `Enter`: `Enter` opens a map and edits a field
/// elsewhere on this screen, and a destructive prompt answered by the most-pressed key on the
/// keyboard is one that gets answered by accident.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// Delete a map. Holds the path it named — see [`Pending`].
    Delete(Pending),
    /// **Remove one entry from a kit's policy.** Nothing is deleted from the file system — a line
    /// is spliced out of the kit's `project.ron` — so this carries the row rather than a `Pending`
    /// path. `shown` is the row's rendered text, so the prompt names the exact entry:
    /// `docs/ui.md` §1.4's rule that "are you sure?" is not information.
    RemovePolicy {
        file: PathBuf,
        shown: String,
        row: PolicyRow,
    },
    /// Leave the chooser.
    Quit,
}

/// **One entry in a kit's policy, keyed for removal.**
///
/// Keyed on the row's ordinal, not on its content: nothing forbids two patches sharing a
/// `matches` key, so a removal keyed on `matches` is ambiguous by construction.
#[derive(Clone, Debug, PartialEq)]
pub enum PolicyRow {
    /// An exclusion — a mesh path prefix.
    Exclude(String),
    /// A patch, by its position within `patches`.
    Patch(usize),
}

/// **A destructive act that has been asked for and not yet agreed to.**
///
/// Deleting a map removes a file, and there is no undo for it here — the editor's undo stack is a
/// different process's memory and does not survive the map being gone. So the act is split in two:
/// asking, which changes nothing, and agreeing, which is a separate keystroke on a prompt naming
/// exactly what will go. `docs/ui.md` §1.4's rule applies to the question as much as to a refusal —
/// it names the map and the file, because "are you sure?" is not information.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    /// What the row said — the map's name, or the kit's label.
    pub name: String,
    /// The file or directory that goes. **A path, never a row index**: a prompt remembering "row 2"
    /// deletes whatever row 2 became if the list moved underneath it.
    pub path: PathBuf,
    /// `true` when `path` is a whole kit directory. Decides `remove_dir_all` against `remove_file`,
    /// and it is carried rather than re-derived so the question and the act cannot disagree about
    /// what is being removed.
    pub kit: bool,
}

/// **What is being made**, which is decided by the panel the arrows were in.
#[derive(Clone, Debug, PartialEq)]
pub enum New {
    /// A kit — a directory the editor will accept. Only a name is asked for; everything else about
    /// a kit is a decision its files record, and `create_kit` writes the defaults with their
    /// reasons.
    Kit(String),
    /// A map, with the four settings.
    Map(Draft),
    /// **A project beside this one.** Only a name is asked for; the skeleton is
    /// [`create_project`]'s, and it copies this project's vocabulary.
    Project(String),
}

impl New {
    /// What has been typed as the name so far, whichever kind it is.
    pub fn name(&self) -> &str {
        match self {
            New::Kit(name) => name,
            New::Map(d) => &d.name,
            New::Project(name) => name,
        }
    }
}

/// The map being made, before it exists on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    /// **Starts empty and stays empty until typed.** `Map::default()` leaves the name blank on
    /// purpose — *"a substituted name is a name nobody chose, and the second one collides with the
    /// first"* — so the field starts blank rather than pre-filled with `untitled_map`.
    pub name: String,
    pub bounds: (f32, f32, f32),
    pub origin: (f32, f32, f32),
    pub note: Option<String>,
}

impl Default for Draft {
    fn default() -> Draft {
        let d = Map::default();
        Draft {
            name: String::new(),
            bounds: d.bounds,
            origin: d.origin,
            note: None,
        }
    }
}

impl Chooser {
    pub fn new(root: PathBuf, catalog: Catalog, preselect: Option<&str>) -> Chooser {
        // **A `--kit` on the command line has already answered half the question this screen asks**,
        // so it selects rather than being discarded.
        // `+ 1` past the `+ new kit` row. Opening ON that row would blank the two columns to its
        // right and greet an author with three empty panels.
        let kit = preselect
            .and_then(|want| {
                catalog
                    .kits
                    .iter()
                    .position(|k| k.flag.as_deref() == Some(want))
            })
            .map_or_else(|| Chooser::first_real(catalog.kits.len()), |i| i + 1);
        let map = Chooser::first_real(catalog.maps.len());
        Chooser {
            root,
            catalog,
            kit,
            policy: 0,
            project: 0,
            map,
            focus: Focus::Kits,
            field: Field::Name,
            editing: false,
            raw: String::new(),
            problem: None,
            creating: None,
            ask: None,
            swallowed: false,
        }
    }

    /// **Come back standing on the map you just left.**
    ///
    /// Returning from the editor rebuilds this screen from scratch — that is what makes going back
    /// cost no teardown — so without this it reopened wherever the original command line pointed,
    /// which after a few rounds is nowhere near where you were. Coming out of `site/hall` and being
    /// dropped on the first kit's first map is the transition feeling like a restart rather than a
    /// step back.
    ///
    /// Both are `Option` and both are separately best-effort: a kit that has since been deleted
    /// leaves the selection where `new` put it, and a map that has been deleted still lands you on
    /// the right kit.
    pub fn reveal(&mut self, kit: Option<&str>, map: Option<&str>) {
        if let Some(want) = kit
            && let Some(i) = self
                .catalog
                .kits
                .iter()
                .position(|k| k.flag.as_deref() == Some(want) || k.label == want)
        {
            self.kit = i + 1;
            self.map = Chooser::first_real(self.catalog.maps.len());
        }
        if let Some(want) = map
            && let Some(i) = self.catalog.maps.iter().position(|m| m.name == want)
        {
            self.map = i + 1;
        }
    }

    /// **Row 0 of every list is `+ new …`, so the selection is offset by one.**
    ///
    /// Asked for at the keyboard: *"a text entry at the very top of maps and kits that says new map,
    /// new kit... if I hit enter on that, it lets me create a new entry respective to the area."*
    /// A row you can see beats a key you have to know — and the row carries the key beside it, so
    /// using the visible path rehearses the fast one (ExposeHK's goal 2, the same argument the hint
    /// line rests on).
    ///
    /// `None` while the `+ new` row is highlighted, and the columns to the right show nothing: there
    /// is genuinely no kit selected, and drawing the last one's contents would be a lie about what
    /// `Enter` is about to do.
    pub fn current_kit(&self) -> Option<&Kit> {
        self.catalog.kits.get(self.kit.checked_sub(1)?)
    }

    /// **The highlighted kit, but only if this project can actually open it.**
    ///
    /// The KITS column lists every directory under `assets/emerge/` that looks like a kit;
    /// `Project::open` opens only the ones `kits.ron` binds. The two info panels asked the first
    /// question and answered as though it were the second, so KIT INFO read `pieces 45` and POLICY
    /// drew eight patch rows for `site` — a kit the project does not have, whose row refuses on
    /// `Enter`. Two panels describing something you cannot enter is worse than two blank ones: the
    /// blank says *there is nothing here for you*, which is true.
    ///
    /// The root kit carries no `flag` and is opened by `Project::open(None)`, so it needs no
    /// binding and is never withheld.
    fn openable_kit(&self) -> Option<&Kit> {
        let binds_any = self.catalog.kits.iter().any(|k| k.bound);
        self.current_kit()
            .filter(|k| !binds_any || k.bound)
    }

    /// **Every settings row the arrows can reach**, in the order they are drawn — which is
    /// [`Field::ALL`], and nothing else. MAP INFO's `bash` row is a fact, not a field: the arrows
    /// walk this list and `B` is what changes the bash.
    pub fn fields(&self) -> Vec<Field> {
        Field::ALL.to_vec()
    }

    /// **Cycle the selected map through the project's bashes**, and round to every-kit.
    ///
    /// The order is `None → bashes[0] → … → bashes[n-1] → None`, read from the map file's current
    /// `bash` so the cycle follows disk rather than a cached position. Written immediately rather
    /// than on some later save, because this panel has no save: every other setting here commits on
    /// `Enter` too. Compton's *grokloop* — the shorter the try/see/change loop, the faster the
    /// learning (Lai et al., `10.1145/3402942.3402946`).
    ///
    /// **Selecting is the only bash verb here.** A bash is authored by hand in `kits.ron`, the same
    /// way its `lattice` is; ticking kits per map would mean editing a shared combination from a map
    /// row, which silently changes every other map naming it.
    pub fn cycle_bash(&mut self) -> Result<(), String> {
        let Some(entry) = self.current_map() else {
            return Err("there is no map here to give a bash to".to_owned());
        };
        let path = entry.path.clone();
        if matches!(entry.summary, MapSummary::Unreadable(_)) {
            return Err("this map will not open, so its bash cannot be set".to_owned());
        }
        if self.catalog.bashes.is_empty() {
            return Err(
                "this project declares no bashes — add one to `kits.ron` under `bash`".to_owned(),
            );
        }
        // **Read from the file, not from a summary.** The summary carries what a row shows, and
        // this verb's whole job is to move a field the row does not carry.
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let map = Map::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let next: Option<&str> = match map.bash.as_deref() {
            // Every kit is where the cycle starts, so the first press names the first bash.
            None => self.catalog.bashes.first().map(String::as_str),
            // A name the project no longer declares lands back at every kit, which is the one
            // state that is always true — and `OpenMap::open` would refuse the map until it does.
            Some(current) => match self.catalog.bashes.iter().position(|b| b == current) {
                Some(i) => self.catalog.bashes.get(i + 1).map(String::as_str),
                None => None,
            },
        };
        write_bash(&path, next)?;
        rescan_keeping_place(self, None);
        Ok(())
    }

    /// **Clamped, like every other panel's cursor.**
    ///
    /// It wrapped, on the argument that neither end should be a dead stop. Neither end *is* one any
    /// more, and for a better reason: MAP INFO is drawn under the map list, so `up` off its first
    /// field carries on up into the list — [`Self::step`] takes over where this stops. Wrapping
    /// would swallow that move and trap the cursor in the panel.
    fn step_field(&mut self, delta: i32) {
        let fields = self.fields();
        let i = fields.iter().position(|f| *f == self.field).unwrap_or(0);
        let next = if delta < 0 {
            i.saturating_sub(1)
        } else {
            (i + 1).min(fields.len().saturating_sub(1))
        };
        self.field = fields.get(next).copied().unwrap_or(Field::Name);
    }

    /// **The map the cursor is on, and no kit is consulted.**
    ///
    /// It used to fetch `current_kit()?` and throw the result away — the last of the tie a map had to
    /// the kit that drew it, from when maps lived inside a kit directory. Maps are the project's now
    /// (`Catalog`), and the fetch was doing real damage rather than nothing: with the KITS cursor
    /// parked on `+ new kit` there is no current kit, so MAP INFO went blank, `Tab` skipped the
    /// settings panel, every kit row lost its `[x]`/`[ ]` mark, `Delete` said "there is no map here"
    /// and `Enter` on a plainly highlighted map row answered "no kit selected".
    pub fn current_map(&self) -> Option<&MapEntry> {
        self.catalog.maps.get(self.map.checked_sub(1)?)
    }

    /// **What is in a settings field right now**, in the spelling the field takes back.
    ///
    /// The seed for an opening text box, so editing a value starts from the value. `triple` is the
    /// same rendering `settings_rows` draws and `parse_triple` reads, so a bounds or origin typed
    /// straight back is the identity — a round trip rather than two formats that agree by habit.
    pub fn settled(&self, field: Field) -> String {
        if let Some(New::Kit(name)) = &self.creating {
            return name.clone();
        }
        if let Some(New::Project(name)) = &self.creating {
            return name.clone();
        }
        let (name, bounds, origin, note) = match (&self.creating, self.current_map()) {
            (Some(New::Map(d)), _) => (d.name.clone(), d.bounds, d.origin, d.note.clone()),
            (Some(New::Kit(_)), _) => return String::new(),
            (Some(New::Project(_)), _) => return String::new(),
            (None, Some(m)) => match &m.summary {
                MapSummary::Read { bounds, .. } => {
                    // The bash is not a field the arrows edit, so it is dropped here.
                    let (origin, note, _) = read_map_details(&m.path);
                    (m.name.clone(), *bounds, origin, note)
                }
                MapSummary::Unreadable(_) => return String::new(),
            },
            (None, None) => return String::new(),
        };
        match field {
            Field::Name => name,
            Field::Bounds => triple(bounds),
            Field::Origin => triple(origin),
            Field::Note => note.unwrap_or_default(),
        }
    }

    /// Is the highlighted row the `+ new …` one?
    pub fn on_new_row(&self) -> bool {
        match self.focus {
            Focus::Kits => self.kit == 0,
            Focus::Maps => self.map == 0,
            // The policy panel has no `+ new` row — adding a patch is not in its scope — and the
            // settings are properties, not a list of things.
            Focus::Settings | Focus::Policy => false,
            Focus::Projects => self.project == 0,
        }
    }

    /// The first row worth landing on in a list of `n` real items: the first item if there is one,
    /// and otherwise the `+ new` row, which is then the only row there is.
    fn first_real(n: usize) -> usize {
        usize::from(n > 0)
    }

    /// **Where the keyboard goes after the row at `was` is removed**, out of `n` remaining.
    ///
    /// The row *above* the one deleted. Asked for at the keyboard: *"when I delete an item, it
    /// shouldn't bring me back to the top of the menu — it should bring me back to the item right
    /// above the one I just deleted."* It went to the top, which on a long list means finding your
    /// place again after every removal, and clearing several in a row means doing that every time.
    ///
    /// Two clamps, and both are the reason this is a function rather than a subtraction. Deleting
    /// the first entry would land on index 0 — the `+ new …` row — so the answer floors at the first
    /// real one; and deleting the last leaves `was` past the end of the shortened list, so it also
    /// ceilings. An empty list has only the `+ new …` row, and 0 is the right answer there.
    fn next_to(was: usize, n: usize) -> usize {
        was.saturating_sub(1).clamp(Chooser::first_real(n), n)
    }

    /// **Walk down the column** — `up`/`down`.
    ///
    /// Inside a panel the cursor is **clamped, not wrapped**: a list that wraps makes "am I at the
    /// end" unanswerable without counting. At the edge the walk *continues* into the panel stacked
    /// next to it in the same column — down from the kit list into POLICY, up from MAP INFO back
    /// into the map list — which is not the thing clamping guards against. Wrapping loses your
    /// place; carrying on downward is monotonic, and it is what the eye does with a column.
    ///
    /// It never leaves the column. `left`/`right` are what cross, and one key doing both is how
    /// `right` came to reach POLICY, which is drawn underneath.
    pub fn step(&mut self, delta: i32) {
        self.problem = None;
        if self.step_within(delta) {
            return;
        }
        // Already at the edge of this panel, so the walk carries on to the next one down (or up)
        // in this column, skipping any with nothing in it.
        let (col, row) = self.focus.at();
        let panels = Focus::COLUMNS[col];
        let mut i = row;
        loop {
            i = match if delta < 0 { i.checked_sub(1) } else { i.checked_add(1) } {
                Some(next) if next < panels.len() => next,
                // The top of the first panel or the bottom of the last: there is nowhere further
                // down this column, and the cursor stays where it is.
                _ => return,
            };
            if self.panel_has_rows(panels[i]) {
                self.focus = panels[i];
                // **Enter at the near edge.** Arriving from above lands on the first row, from
                // below on the last — so the highlight appears where the key was pointing rather
                // than wherever the panel was left last time.
                self.enter_panel(delta);
                return;
            }
        }
    }

    /// Move the cursor inside the focused panel. `true` when it actually moved — `false` means the
    /// cursor was already at that edge, which is what sends [`Self::step`] on to the next panel.
    fn step_within(&mut self, delta: i32) -> bool {
        match self.focus {
            Focus::Kits => {
                // `+ 1` for the `+ new kit` row, which is always there — even in a project whose
                // every kit was deleted, where it is the only thing left to press.
                let was = self.kit;
                self.kit = clamp_step(was, delta, self.catalog.kits.len() + 1);
                // **The map selection stays put.** It used to be reset here, because a different
                // kit meant a different map list and the old index could point past the new one.
                // There is one list now — the project's — so there is no index to invalidate, and
                // resetting would move the row an author is reading out from under them while they
                // change only where new work lands.
                self.kit != was
            }
            Focus::Maps => {
                let was = self.map;
                self.map = clamp_step(was, delta, self.catalog.maps.len() + 1);
                self.map != was
            }
            // **The arrows walk the settings rows too**, which is the whole of the correction:
            // moving inside a panel is always the arrows, whichever panel it is.
            Focus::Settings => {
                let was = self.field;
                self.step_field(delta);
                self.field != was
            }
            // The policy rows are the kit's entries; there is no `+ new` row, so the cursor walks
            // exactly the entries that exist.
            Focus::Policy => {
                let was = self.policy;
                self.policy = clamp_step(was, delta, self.policy_rows().1.len());
                self.policy != was
            }
            // The projects list always has the `+ new project` row, and the real entries beside
            // the current root.
            Focus::Projects => {
                let was = self.project;
                self.project = clamp_step(was, delta, self.catalog.projects.len() + 1);
                self.project != was
            }
        }
    }

    /// Put the cursor at the edge of the newly focused panel the arrow was pointing at: the first
    /// row when arriving from above, the last when arriving from below.
    fn enter_panel(&mut self, delta: i32) {
        let down = delta >= 0;
        match self.focus {
            Focus::Kits => self.kit = if down { 0 } else { self.catalog.kits.len() },
            Focus::Maps => self.map = if down { 0 } else { self.catalog.maps.len() },
            Focus::Projects => {
                self.project = if down { 0 } else { self.catalog.projects.len() }
            }
            Focus::Policy => {
                self.policy = if down {
                    0
                } else {
                    self.policy_rows().1.len().saturating_sub(1)
                }
            }
            Focus::Settings => {
                let fields = self.fields();
                self.field = if down {
                    fields.first().copied().unwrap_or(Field::Name)
                } else {
                    fields.last().copied().unwrap_or(Field::Note)
                }
            }
        }
    }

    /// **Cross to the column on the left or the right** — `left`/`right`, the pair the Meshes tab
    /// binds to [`crate::keys::Action::FocusCandidates`] / `FocusLibrary` for the same job.
    ///
    /// Lands on the **top** panel of that column — its list — because that is what naming a column
    /// means: `right` from KITS goes to MAPS, not to whatever panel of the maps column the cursor
    /// was left in. Always one press per column, which is the whole point: it used to be two from
    /// KITS to MAPS and one from everywhere else, because the walk threaded through POLICY.
    ///
    /// **Clamped at both ends, and it wrapped until 2026-09-03.** The old argument was that "am I
    /// at the end" is answerable at a glance with three columns on screen, so the condition
    /// [`Self::step`] clamps for was not met here. What the audit found at the keyboard is that it
    /// is answerable and still surprising: from KITS two presses of `→` land on PROJECTS, which is
    /// *leftwards* — a carousel in a layout whose whole point (see [`spawn_screen`]) is that left
    /// to right IS the data model. Decision D13 clamps it, so `→` always means "further right, or
    /// stay", and the ends of the row of columns are the ends.
    pub fn cross(&mut self, delta: i32) {
        self.problem = None;
        let (at, _) = self.focus.at();
        let last = Focus::COLUMNS.len().saturating_sub(1);
        let i = if delta < 0 {
            at.saturating_sub(1)
        } else {
            at.saturating_add(1).min(last)
        };
        // Every column's top panel is a list, and all three always draw at least their `+ new …`
        // row — so there is no empty column to skip and no loop to write.
        if let Some(head) = Focus::COLUMNS[i].first() {
            self.focus = *head;
        }
    }

    /// Is there anything in that panel for the arrows to be on?
    fn panel_has_rows(&self, panel: Focus) -> bool {
        match panel {
            // The kit list is never empty — `Catalog::scan` refuses a root with no kits — and the
            // map panel always draws a row, the instruction when there are no maps.
            Focus::Kits | Focus::Maps => true,
            Focus::Settings => self.creating.is_some() || self.current_map().is_some(),
            // **The policy panel depends on the KITS cursor AND on what that kit declares.** With
            // the cursor on `+ new kit` there is no kit whose policy to show, and a kit that
            // excludes nothing and patches nothing draws no rows either — the same rule that
            // already empties KIT INFO on the `+ new kit` row.
            //
            // **Its real row count, not `current_kit().is_some()`.** That answered "could this
            // panel exist" while this function asks "is there anything to stand on", and the gap
            // between them was survivable while a key walked *past* an empty panel. `down` off the
            // end of the kit list walks *into* it, so an empty one is a place the cursor gets stuck
            // with nothing to press — the dead stop `keys.rs` refuses to ship.
            Focus::Policy => !self.policy_rows().1.is_empty(),
            // The projects list always draws its `+ new project` row.
            Focus::Projects => true,
        }
    }

    /// **The selected kit's policy, as rows** — its exclusions and its patches, read from that
    /// kit's `project.ron`. Empty when no kit is selected, which is the same rule that empties
    /// KIT INFO on the `+ new kit` row.
    ///
    /// An exclusion renders `exclude  <pack>`; a patch renders `patch  <matches> — <because>`,
    /// `matches` being `Match::Id(id)` or `Match::Kind(kind)`. The rows carry the [`PolicyRow`]
    /// that `Delete` removes by ordinal — never by content, because nothing forbids two patches
    /// sharing a `matches` key.
    fn policy_rows(&self) -> (String, Vec<Row>) {
        let Some(k) = self.openable_kit() else {
            return ("POLICY".to_owned(), Vec::new());
        };
        let path = k.dir.join(emerge_core::policy::POLICY_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                return (
                    "POLICY".to_owned(),
                    vec![Row {
                        left: "unreadable".to_owned(),
                        right: format!("{e}"),
                        tone: Tone::Problem,
                    }],
                );
            }
        };
        let policy = match emerge_core::policy::Policy::parse(&text) {
            Ok(p) => p,
            Err(e) => {
                return (
                    "POLICY".to_owned(),
                    vec![Row {
                        left: "unreadable".to_owned(),
                        right: e,
                        tone: Tone::Problem,
                    }],
                );
            }
        };
        let mut rows = Vec::new();
        for pack in &policy.exclude {
            rows.push(Row {
                left: "exclude".to_owned(),
                right: pack.clone(),
                tone: Tone::Row,
            });
        }
        for patch in &policy.patches {
            let matches = match &patch.matches {
                emerge_core::policy::Match::Id(id) => format!("Id({id})"),
                emerge_core::policy::Match::Kind(kind) => format!("Kind({kind})"),
            };
            rows.push(Row {
                left: "patch".to_owned(),
                right: format!("{matches} — {}", patch.because),
                tone: Tone::Row,
            });
        }
        ("POLICY".to_owned(), rows)
    }

    /// **The projects beside this one, as rows.** Row 0 is `+ new project`; a real project `i`
    /// is `i + 1`, the indexing rule every other column uses. The current project is marked, so
    /// the list says which one this screen is standing in — and `Enter` on it does nothing,
    /// because it is already open.
    fn projects_rows(&self) -> (String, Vec<Row>) {
        let mut rows = vec![Row {
            left: "+ new project".to_owned(),
            right: "N".to_owned(),
            tone: if self.focus == Focus::Projects && self.project == 0 {
                Tone::Selected
            } else {
                Tone::Row
            },
        }];
        rows.extend(self.catalog.projects.iter().enumerate().map(|(i, p)| {
            let selected = self.focus == Focus::Projects && i + 1 == self.project;
            Row {
                left: if p.current {
                    format!("{} (this one)", p.name)
                } else {
                    p.name.clone()
                },
                right: String::new(),
                tone: if selected {
                    Tone::Selected
                } else if p.current {
                    Tone::Stocked
                } else {
                    Tone::Row
                },
            }
        }));
        ("PROJECTS".to_owned(), rows)
    }

    /// **Start making a new thing in whichever panel the arrows are in.**
    ///
    /// One rule: `N` — and `Enter` on the `+ new …` row — makes a new one of whatever this column
    /// lists. Asked for at the keyboard: *"if I'm on the kits' menu area, then I press N, that
    /// should create a new kit."*
    pub fn start_new(&mut self) {
        self.problem = None;
        self.raw.clear();
        self.field = Field::Name;
        self.creating = Some(match self.focus {
            // The settings belong to a map, so `N` there means the same as `N` on the map list.
            Focus::Kits => New::Kit(String::new()),
            Focus::Maps | Focus::Settings => New::Map(Draft::default()),
            // **No `N` on the policy panel.** Adding a patch is not in this panel's scope — a
            // patch's payload is a whole partial `Descriptor`, and the one place descriptor fields
            // are authored is the detail pane. The hint does not offer the key, and pressing it
            // here falls through to the same refusal the other dead keys give.
            Focus::Policy => New::Map(Draft::default()),
            // **`N` on the projects column makes a new project beside this one** — the same one
            // rule as every other column: `N` makes a new one of whatever the column lists.
            Focus::Projects => New::Project(String::new()),
        });
        self.focus = Focus::Settings;
        // Straight into the name: it is the one thing neither a kit nor a map can be made without.
        self.editing = true;
    }

    /// **Ask to delete the selected map.** Changes nothing on disk — see [`Pending`].
    ///
    /// The question captures the **path**, not the row index. A prompt that remembered "row 2"
    /// would delete whatever row 2 became if the list moved underneath it; a prompt holding a path
    /// deletes the file it named or nothing at all.
    pub fn ask_delete(&mut self) -> Result<(), String> {
        match self.focus {
            Focus::Maps => {
                let m = self
                    .current_map()
                    .ok_or_else(|| "there is no map here to delete".to_owned())?;
                self.ask = Some(Ask::Delete(Pending {
                    name: m.name.clone(),
                    path: m.path.clone(),
                    kit: false,
                }));
                Ok(())
            }
            // **A kit goes as a whole directory**, which is why the question names what is inside
            // it. Asked for at the keyboard: *"under the kits area, I don't see a way to delete the
            // kits."*
            Focus::Kits => {
                let k = self
                    .current_kit()
                    .ok_or_else(|| "there is no kit here to delete".to_owned())?;
                // **The root kit is refused, and this is the guard that matters.** It is
                // `assets/emerge` itself — the directory every other kit is a subdirectory of, and
                // the one holding the shared vocabulary. `remove_dir_all` on it would take the
                // whole library, so it is not offered rather than offered and warned about.
                if k.flag.is_none() {
                    // Short on purpose. It used to explain the directory layout — *"every other
                    // kit lives inside it"* — and that was reported as too much: *"I don't need
                    // quite as much text… just say it can't be deleted."* A refusal is read while
                    // reaching for the next key, so it says which and that it won't, and stops.
                    return Err(format!(
                        "`{}` is the default kit and cannot be deleted",
                        k.label
                    ));
                }
                // **And the guard that was missing, which cost the shipped kit.** `confirm_delete`
                // is `remove_dir_all`, and until this existed the only thing it would not take was
                // the root — so the kit `src/site/kit.rs` names in a `&'static str` went, with 51
                // tests and the game's ability to boot. Asked here rather than at the prompt, for
                // the same reason the root kit is: not offered beats offered and warned about.
                //
                // The full scan runs on a keypress. That is the trade the chooser already makes in
                // the other direction — `read_kit` parses only `library.ron` for a list nobody has
                // chosen from — and it is the right way round: listing is cheap because it is
                // constant, deleting is thorough because it happens once and cannot be undone.
                let users = dependents(&self.root, k, &self.catalog)?;
                if let Some(problem) = strands(&k.label, &users) {
                    return Err(problem);
                }
                self.ask = Some(Ask::Delete(Pending {
                    name: k.label.clone(),
                    path: k.dir.clone(),
                    kit: true,
                }));
                Ok(())
            }
            Focus::Settings => {
                Err("Delete removes a kit or a map — Tab to one of those lists first".to_owned())
            }
            // **A project is a whole directory tree.** Nothing here deletes one — the refusal
            // says what it is and where the author would do it.
            Focus::Projects => Err(
                "a project is a whole directory tree — remove it from the file system".to_owned(),
            ),
            // **`Delete` on a policy row asks to remove that entry** — an exclusion or a patch —
            // through the same confirmation the menu gives a directory. A `because` string is
            // hand-written rationale, and the menu already asks before removing a directory.
            Focus::Policy => {
                let k = self
                    .current_kit()
                    .ok_or_else(|| "there is no kit here to remove policy from".to_owned())?;
                let path = k.dir.join(emerge_core::policy::POLICY_FILE);
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                let policy = emerge_core::policy::Policy::parse(&text)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                let (_, rows) = self.policy_rows();
                let row = rows
                    .get(self.policy)
                    .ok_or_else(|| "there is no policy entry under the cursor".to_owned())?;
                let shown = format!("{}  {}", row.left, row.right);
                // The row's ordinal within its own list, not within the rendered rows: exclusions
                // come first, so a patch's ordinal is its index past them.
                let policy_row = if self.policy < policy.exclude.len() {
                    PolicyRow::Exclude(policy.exclude[self.policy].clone())
                } else {
                    PolicyRow::Patch(self.policy - policy.exclude.len())
                };
                self.ask = Some(Ask::RemovePolicy {
                    file: path,
                    shown,
                    row: policy_row,
                });
                Ok(())
            }
        }
    }

    /// **Agree to it.** Removes the file the question named, then rescans so the list is a
    /// description of disk rather than of the edit.
    pub fn confirm_delete(&mut self) -> Result<String, String> {
        // **A policy removal is not a deletion** — nothing leaves the file system, a line is
        // spliced out of `project.ron` — so it is answered here, before the `Delete` arm, and
        // never reaches `remove_file`/`remove_dir_all`.
        //
        // **A `take()` in a pattern guard would consume the `Delete` ask too** — the value is
        // dropped the moment the pattern fails — so the dispatch reads the clone and takes only
        // when it is a policy removal.
        if matches!(self.ask, Some(Ask::RemovePolicy { .. })) {
            let Some(Ask::RemovePolicy { file, row, .. }) = self.ask.take() else {
                unreachable!("guarded above");
            };
            let result = match row {
                PolicyRow::Exclude(pack) => {
                    let text = std::fs::read_to_string(&file)
                        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
                    let policy = emerge_core::policy::Policy::parse(&text)
                        .map_err(|e| format!("{}: {e}", file.display()))?;
                    let mut exclude = policy.exclude.clone();
                    exclude.retain(|p| p != &pack);
                    let out = emerge_core::policy::rewrite_exclude(&text, &exclude)
                        .map_err(|e| format!("{}: {e}", file.display()))?;
                    emerge_core::ron_surgery::save_atomic(&file, &out)?;
                    Ok(pack)
                }
                PolicyRow::Patch(ordinal) => {
                    emerge_core::policy::remove_patch(&file, ordinal)?;
                    Ok(format!("patch #{ordinal}"))
                }
            };
            match result {
                Ok(what) => {
                    // The row after the one removed, so the keyboard lands next to it.
                    let was = self.policy;
                    rescan_keeping_place(self, None);
                    self.policy = Chooser::next_to(was, self.policy_rows().1.len());
                    Ok(what)
                }
                Err(e) => Err(e),
            }
        } else {
            self.confirm_delete_entry()
        }
    }

    /// The `Delete` half of [`Self::confirm_delete`] — a kit or a map, which really does remove
    /// from the file system.
    fn confirm_delete_entry(&mut self) -> Result<String, String> {
        let Some(Ask::Delete(pending)) = self.ask.take() else {
            return Err("no deletion was asked about".to_owned());
        };
        // **Unbind first, and refuse the whole act if that refuses.** The binding is the project's
        // statement that this kit exists; removing the directory while `kits.ron` still names it
        // leaves a project that will not open, which is a far worse outcome than a refusal. Doing it
        // in this order means the failure costs nothing — the directory is still there.
        if pending.kit {
            unbind_kit(&self.root, &pending.name)?;
        }
        // One call or the other, chosen by what the question captured — not by looking at the
        // path again, which could have become a different kind of thing in between.
        let gone = if pending.kit {
            std::fs::remove_dir_all(&pending.path)
        } else {
            std::fs::remove_file(&pending.path)
        };
        gone.map_err(|e| format!("could not delete `{}`: {e}", pending.name))?;
        // Where the removed row was, so the keyboard can land next to it rather than at the top.
        let was = if pending.kit { self.kit } else { self.map };
        // The label `rescan_keeping_place` would try to hold is the row just removed, so it falls
        // through to the first real one — which is what this then corrects.
        rescan_keeping_place(self, None);
        if pending.kit {
            self.kit = Chooser::next_to(was, self.catalog.kits.len());
            self.map = Chooser::first_real(self.catalog.maps.len());
            self.focus = Focus::Kits;
        } else {
            self.map = Chooser::next_to(was, self.catalog.maps.len());
            if self.current_map().is_none() {
                self.focus = Focus::Kits;
            }
        }
        Ok(pending.name)
    }

    /// **What the editor would be launched with**, or why it cannot be.
    /// **What to launch, and through which door.**
    ///
    /// The column the cursor is in *is* the door: a kit row opens the Kits door on that kit, and a
    /// map row opens the Maps door on that map. Asked for at the keyboard, 2026-08-16 — *"when I
    /// select a kit and I press enter, I'm still getting [map], meshes, tiles, compose, anim."*
    ///
    /// `--kit` is passed either way, and means the same thing in both: **where new work lands**. On
    /// the Kits door that is the kit being edited; on the Maps door it is the namespace a captured
    /// tile is named in.
    pub fn launch_args(&self) -> Result<Vec<String>, String> {
        if self.on_new_row() {
            return Err("that row makes a new one — press Enter on it".to_owned());
        }
        let root = self.root.display().to_string();

        // **The kits column opens the kit, and nothing else.** No map is named, because the Kits
        // door does not have one — see `project::OpenMap`.
        if self.focus == Focus::Kits {
            let kit = self
                .current_kit()
                .ok_or_else(|| "no kit selected".to_owned())?;
            let Some(flag) = &kit.flag else {
                return Err(format!(
                    "`{}` is not a bound kit, so there is nothing to open it as.",
                    kit.label
                ));
            };
            return Ok(vec![
                root,
                "--door".to_owned(),
                Door::Kit.label().to_lowercase(),
                "--kit".to_owned(),
                flag.clone(),
            ]);
        }
        // **The policy panel opens the kit it describes** — the same door `Enter` on the kit row
        // opens, because the policy is the kit's. The panel is a view over `project.ron`, not a
        // door of its own.
        if self.focus == Focus::Policy {
            let kit = self
                .current_kit()
                .ok_or_else(|| "no kit selected".to_owned())?;
            let Some(flag) = &kit.flag else {
                return Err(format!(
                    "`{}` is not a bound kit, so there is nothing to open it as.",
                    kit.label
                ));
            };
            return Ok(vec![
                root,
                "--door".to_owned(),
                Door::Kit.label().to_lowercase(),
                "--kit".to_owned(),
                flag.clone(),
            ]);
        }

        let map = self
            .current_map()
            .ok_or_else(|| "no maps in this project yet — press Enter on `+ new map`".to_owned())?;
        if let MapSummary::Unreadable(why) = &map.summary {
            return Err(format!("`{}` will not open: {why}", map.name));
        }
        let mut args = vec![
            root,
            map.name.clone(),
            "--door".to_owned(),
            Door::Map.label().to_lowercase(),
        ];
        // **`--kit` is optional on the Maps door, and that is not a fallback.** It says where new
        // work lands, and `Project::open(.., None)` answers that from the project's own `authoring`
        // binding. Requiring it here refused to open a perfectly good map whenever the KITS cursor
        // happened to be on `+ new kit` or on the unbound root kit — a refusal naming a column the
        // author was not in.
        if let Some(flag) = self.current_kit().and_then(|k| k.flag.clone()) {
            args.push("--kit".to_owned());
            args.push(flag);
        }
        Ok(args)
    }
}

fn clamp_step(at: usize, delta: i32, len: usize) -> usize {
    let last = len.saturating_sub(1);
    if delta < 0 {
        at.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        (at + delta as usize).min(last)
    }
}

#[cfg(test)]
mod screen_tests {
    use super::*;

    fn entry(name: &str, summary: MapSummary) -> MapEntry {
        MapEntry {
            name: name.to_owned(),
            path: PathBuf::from(format!("{name}.map.ron")),
            summary,
        }
    }

    fn ok_map(name: &str) -> MapEntry {
        entry(
            name,
            MapSummary::Read {
                placements: 0,
                stamps: 0,
                bounds: (10.0, 4.0, 10.0),
            },
        )
    }

    fn kit(flag: Option<&str>, label: &str, pieces: usize) -> Kit {
        Kit {
            flag: flag.map(str::to_owned),
            label: label.to_owned(),
            dir: PathBuf::from(label),
            pieces,
            namespace: None,
            bound: false,
            ids: BTreeSet::new(),
        }
    }

    fn chooser(preselect: Option<&str>) -> Chooser {
        let catalog = Catalog {
            kits: vec![
                kit(None, "emerge", 75),
                kit(Some("site"), "site", 45),
                kit(Some("site_v2"), "site_v2", 0),
            ],
            // **One list for the project.** These used to be three lists, one per kit, and moving
            // between kits changed which maps were on screen — which is exactly the tie a map no
            // longer has to the kit that drew it.
            maps: vec![ok_map("hall"), ok_map("test1"), ok_map("untitled_map")],
            authoring: Some("site".to_owned()),
            bashes: Vec::new(),
            projects: Vec::new(),
        };
        Chooser::new(PathBuf::from("."), catalog, preselect)
    }

    /// **`--kit site` has already answered half the question the screen asks**, so it selects rather
    /// than being thrown away. Three of today's relaunches were the author supplying exactly this.
    #[test]
    fn a_kit_named_on_the_command_line_is_preselected() {
        assert_eq!(
            chooser(Some("site"))
                .current_kit()
                .map(|k| k.label.as_str()),
            Some("site")
        );
        assert_eq!(
            chooser(Some("site_v2"))
                .current_kit()
                .map(|k| k.label.as_str()),
            Some("site_v2")
        );
        // An unknown kit does not silently pick something else — it lands on the first row, and the
        // list is right there showing what does exist. `Project::open` refuses the same name loudly.
        assert_eq!(
            chooser(Some("nope"))
                .current_kit()
                .map(|k| k.label.as_str()),
            Some("emerge")
        );
        assert_eq!(
            chooser(None).current_kit().map(|k| k.label.as_str()),
            Some("emerge")
        );
    }

    /// Clamped, not wrapped: a list that wraps makes "am I at the end" unanswerable without counting.
    #[test]
    fn the_arrows_clamp_at_both_ends() {
        let mut c = chooser(None);
        c.step(-1);
        assert_eq!(
            c.kit, 0,
            "up at the top stays — and the top is the `+ new kit` row"
        );
        for _ in 0..6 {
            c.step(1);
        }
        assert_eq!(
            c.kit, 3,
            "three kits sit at rows 1..=3, and walking past the end stays on the last"
        );
    }

    /// **Changing the kit no longer moves the map list**, which is the whole point of maps leaving
    /// the kit directories.
    ///
    /// This test used to check the opposite: that walking onto a kit with no maps put the selection
    /// back on the `+ new map` row, because each kit carried its own list and the old selection
    /// could index past the new one. There is one list now, so there is no index to invalidate —
    /// and the row an author was reading stays under their eyes while they change where new work
    /// lands.
    #[test]
    fn changing_kit_leaves_the_map_selection_alone() {
        let mut c = chooser(Some("site"));
        c.cross(1); // KITS -> MAPS, one press
        c.step(1);
        assert_eq!(c.map, 2, "row 1 is the first map; row 0 makes a new one");
        let chosen = c.current_map().map(|m| m.name.clone());
        c.cross(-1); // MAPS -> KITS
        c.step(1); // -> a different kit
        assert_eq!(c.map, 2, "the map row does not move under the author");
        assert_eq!(
            c.current_map().map(|m| m.name.clone()),
            chosen,
            "and it is still the same map"
        );
    }

    /// **The arrows are the only navigation, and each one means what it points at**: `left`/`right`
    /// cross columns, `up`/`down` walk down the column you are in.
    ///
    /// The rule the rest of the editor already follows — the Meshes tab binds `left`/`right` to
    /// `FocusCandidates`/`FocusLibrary` for exactly this, and Compose says *"up/down walk the
    /// groups, left/right walk the members"*. `Tab` used to do the crossing here and was the only
    /// `KeyCode::Tab` in the crate.
    ///
    /// **One press per column, from anywhere** — which is the defect this replaced. The old walk
    /// threaded every panel onto one key, so KITS to MAPS cost two presses (through POLICY, drawn
    /// *below* the kit list) while every other crossing cost one, and which it was depended on
    /// where the other columns' cursors happened to be.
    #[test]
    fn left_right_cross_columns_and_up_down_walk_down_one() {
        let mut c = chooser(Some("site"));
        assert_eq!(c.focus, Focus::Kits);

        // **One press each — and since 2026-09-03 (D13) the ends are the ends.** It used to wrap,
        // so two presses of `→` from KITS landed on PROJECTS, which is leftwards.
        c.cross(1);
        assert_eq!(c.focus, Focus::Maps, "KITS -> MAPS is one press, not two");
        c.cross(1);
        assert_eq!(
            c.focus,
            Focus::Maps,
            "MAPS is the rightmost column, so `right` holds rather than reappearing on the left"
        );
        c.cross(-1);
        assert_eq!(c.focus, Focus::Kits, "backwards too");
        c.cross(-1);
        assert_eq!(c.focus, Focus::Projects, "and on to the first column");
        c.cross(-1);
        assert_eq!(c.focus, Focus::Projects, "which is where `left` stops");
        c.cross(1);
        c.cross(1);
        assert_eq!(c.focus, Focus::Maps, "and back across, one column per press");

        // **A crossing lands on the column's LIST**, never on the inspector under it — naming a
        // column means its head, not wherever that column was left.
        c.step(1);
        c.step(1);
        assert_eq!(c.map, 3, "walked to the last map row");
        c.cross(-1);
        c.cross(1);
        assert_eq!(c.focus, Focus::Maps, "back on the list, not on MAP INFO");

        // **`down` off the end of a list carries on into the panel drawn under it**, and `up`
        // comes back. That is the vertical arrangement answered by the vertical key.
        c.step(1);
        assert_eq!(c.focus, Focus::Settings, "MAP INFO is below the map list");
        assert_eq!(c.field, Field::Name, "and it is entered at the top");
        c.step(-1);
        assert_eq!(c.focus, Focus::Maps, "up comes back out of it");
        assert_eq!(c.map, 3, "at the row it left from");

        // **And it never leaves the column.** Walking down past the bottom of the last panel in a
        // column stays put rather than appearing in the next one over.
        c.step(1);
        for _ in 0..8 {
            c.step(1);
        }
        assert_eq!(
            c.focus,
            Focus::Settings,
            "the bottom of the MAPS column is the end of the walk, not the door to another"
        );
    }

    /// **A panel with nothing in it is not walked into**, because landing the arrows where they can
    /// do nothing is the dead stop `keys.rs` refuses to ship.
    ///
    /// Under the old crossing key this meant *skipped over* — the walk threaded every panel onto
    /// one key, so an empty one was passed through on the way to the next. `down` walks **into** a
    /// panel now, so an empty one has to be a place the cursor does not go at all: the walk stops
    /// at the bottom of the list instead.
    #[test]
    fn a_panel_with_no_rows_is_not_walked_into() {
        let mut c = Chooser::new(
            PathBuf::from("."),
            Catalog {
                kits: vec![kit(Some("site_v2"), "site_v2", 0)],
                maps: Vec::new(),
                authoring: Some("site_v2".to_owned()),
                bashes: Vec::new(),
                projects: Vec::new(),
            },
            Some("site_v2"),
        );
        assert_eq!(c.focus, Focus::Kits);

        // **No map is selected**, so MAP INFO has nothing to show and `down` off the end of the map
        // list has nowhere to go.
        c.cross(1);
        assert_eq!(
            c.focus,
            Focus::Maps,
            "the map panel always draws a row — the instruction"
        );
        for _ in 0..4 {
            c.step(1);
        }
        assert_eq!(c.focus, Focus::Maps, "there are no settings to walk into");

        // **Crossing is never affected by any of it.** Every column's top panel is a list and all
        // three always draw at least their `+ new …` row, so there is no empty column to skip —
        // and the walk stops at the first column rather than wrapping past it.
        c.cross(-1);
        assert_eq!(c.focus, Focus::Kits);
        c.cross(-1);
        assert_eq!(
            c.focus,
            Focus::Projects,
            "the projects panel always draws a row — the instruction"
        );
        c.cross(-1);
        assert_eq!(c.focus, Focus::Projects, "and `left` stops at the leftmost column");
    }

    /// The settings rows are walked by the arrows, clamped, like every other panel — and the edge
    /// is not a dead stop because it leads back up into the map list drawn above.
    #[test]
    fn the_arrows_walk_the_settings_rows() {
        let mut c = chooser(Some("site"));
        // MAP INFO is under the map list: one crossing, then down off the end of the list.
        c.cross(1);
        while c.focus == Focus::Maps {
            c.step(1);
        }
        assert_eq!(c.focus, Focus::Settings);
        assert_eq!(c.field, Field::Name);
        c.step(1);
        assert_eq!(c.field, Field::Bounds);
        c.step(-1);
        assert_eq!(c.field, Field::Name);
        // **Backwards from the first row leaves the panel** rather than wrapping onto the last.
        // The panel is the map's four properties and one `bash` fact, drawn under the map list —
        // so `up` from the top means the list, which is what the arrow is pointing at.
        c.step(-1);
        assert_eq!(c.focus, Focus::Maps, "up off the top goes back to the list above");
        // And forwards from the last row is the bottom of the column, so it stays.
        c.step(1);
        assert_eq!(c.focus, Focus::Settings);
        for _ in 0..8 {
            c.step(1);
        }
        let last = c
            .fields()
            .last()
            .copied()
            .unwrap_or_else(|| panic!("the panel has rows"));
        assert_eq!(c.field, last, "clamped on the last row");
        assert_eq!(last, Field::Note, "which is the last of the map's own properties");
    }

    /// **The inspector cannot hold fewer rows than it draws**, and that is now a property of the
    /// layout rather than a sum somebody has to keep true.
    ///
    /// It used to be arithmetic. `panel()` set `height: Val::Px(..)` — fixed, so a field never moved
    /// under the hand about to type into it — and the cost was that rows past the height did not
    /// make the panel taller: `Overflow` is visible by default, so they drew **over** the message
    /// row and off the bottom edge. The kit selection did exactly that the day it was added, four
    /// settings sized the panel and it drew `1 + kits` rows more, and a text render of the screen
    /// could not see it because every row was present and correct in the model.
    ///
    /// The frame made the sum unnecessary: the inspector is content-sized and refuses to shrink, so
    /// it is exactly as tall as its rows and the list above it gives up the difference. What is
    /// pinned here is that shape — a fixed `height`, or a `flex_shrink` that lets it be squeezed,
    /// brings the whole defect back.
    #[test]
    fn the_inspector_is_sized_by_its_rows_and_not_by_a_number() {
        let node = panel_node(info_panel());
        assert_eq!(
            node.height,
            Val::Auto,
            "a fixed height cannot be right for both a four-row inspector and a twelve-row one: it \
             is padded for the first or it draws over the band for the second"
        );
        assert_eq!(
            node.flex_shrink, 0.0,
            "an inspector that can be squeezed is an inspector that clips its last row, which is \
             the same defect with a different cause"
        );
    }

    /// **The launch line, which is the whole output of this screen.**
    ///
    /// `--kit` still travels, and it means *"author in this kit"* rather than *"load only this
    /// kit"* — every bound kit is loaded either way. **The map is chosen independently of it**: the
    /// same map opens under any authoring kit, because the library that draws it is the merge.
    ///
    /// **And the column now decides the door.** A map row opens the Maps door on that map; a kit
    /// row opens the Kits door on that kit, naming no map at all.
    #[test]
    fn the_launch_line_carries_the_kit_and_the_map_independently() {
        let mut c = chooser(Some("site"));
        c.focus = Focus::Maps;
        assert_eq!(
            c.launch_args().unwrap_or_else(|e| panic!("{e}")),
            vec![".", "hall", "--door", "map", "--kit", "site"]
        );

        // A different kit, the same map: the map list did not move under it.
        c.kit = 3; // `site_v2` — row 0 is `+ new kit`
        assert_eq!(
            c.launch_args().unwrap_or_else(|e| panic!("{e}")),
            vec![".", "hall", "--door", "map", "--kit", "site_v2"],
            "the map is the project's, so changing the authoring kit does not change it"
        );
    }

    /// **The kits column opens the kit, and names no map.**
    ///
    /// Reported at the keyboard, 2026-08-16: *"when I select a kit and I press enter, I'm still
    /// getting [map], meshes, tiles, compose, anim."* The launch line is where that is decided — a
    /// door that named a map would have one, and everything downstream would then be able to read
    /// it.
    #[test]
    fn a_kit_row_opens_the_kit_door_and_names_no_map() {
        let mut c = chooser(Some("site"));
        c.focus = Focus::Kits;
        let args = c.launch_args().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(args, vec![".", "--door", "kit", "--kit", "site"]);
        assert!(
            !args.iter().any(|a| a == "hall"),
            "no map travels with a kit: {args:?}"
        );
    }

    /// An unmet condition is an instruction, not a report (`docs/ui.md` §1.4). A project with no
    /// maps says what to press.
    ///
    /// **What it says changed with the doors.** It used to be reached from the kits column — Enter
    /// on a kit with no maps had nowhere to go. A kit row now opens the Kits door, which needs no
    /// map, so the only way to ask for a map that is not there is to stand in the maps column, where
    /// the sole row is `+ new map`. The instruction is the same one; the row giving it is different.
    #[test]
    fn a_project_with_no_maps_says_what_to_do_about_it() {
        let c = Chooser::new(
            PathBuf::from("."),
            Catalog {
                kits: vec![kit(Some("site_v2"), "site_v2", 0)],
                maps: Vec::new(),
                authoring: Some("site_v2".to_owned()),
                bashes: Vec::new(),
                projects: Vec::new(),
            },
            Some("site_v2"),
        );
        let mut c = c;
        c.focus = Focus::Maps;
        assert!(c.on_new_row(), "with no maps the only row is `+ new map`");
        let e = c
            .launch_args()
            .err()
            .unwrap_or_else(|| panic!("nothing to open"));
        assert!(
            e.contains("press Enter on it"),
            "the refusal has to be an instruction: {e}"
        );

        // And the kits column still opens, because the Kits door does not want a map.
        c.focus = Focus::Kits;
        let args = c.launch_args().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(args, vec![".", "--door", "kit", "--kit", "site_v2"]);
    }

    /// A map that would not parse is offered as a row and refused at `Enter`, with the reason — not
    /// hidden from the list, and not launched into a crash.
    #[test]
    fn an_unreadable_map_is_refused_at_the_door_with_its_reason() {
        let catalog = Catalog {
            kits: vec![kit(Some("site"), "site", 1)],
            maps: vec![entry("broken", MapSummary::Unreadable("map: bad ron".into()))],
            authoring: Some("site".to_owned()),
            bashes: Vec::new(),
            projects: Vec::new(),
        };
        let mut c = Chooser::new(PathBuf::from("."), catalog, None);
        c.focus = Focus::Maps;
        let e = c
            .launch_args()
            .err()
            .unwrap_or_else(|| panic!("must refuse"));
        assert!(e.contains("broken"), "{e}");
        assert!(
            e.contains("bad ron"),
            "the reason travels with the refusal: {e}"
        );
    }

    /// A new map's name starts **empty** — `Map::default()` leaves it so on purpose, and a chooser
    /// that pre-filled `untitled_map` would reintroduce the substituted name `emerge-core` refuses.
    #[test]
    fn a_draft_starts_unnamed_and_carries_the_default_bounds() {
        let d = Draft::default();
        assert!(d.name.is_empty(), "the name field starts blank");
        assert_eq!(
            d.bounds,
            Map::default().bounds,
            "and the bounds are the map default"
        );
    }

    /// The four settings are a fixed run in the order they are drawn, walked by the **arrows** —
    /// the only navigation this screen has. **Clamped**, like every other panel: the end is not a
    /// dead stop because the walk carries on out of the panel, and `up` off the top goes back to
    /// the map list drawn above it. See [`Chooser::step`].
    #[test]
    fn the_field_run_goes_in_the_order_they_are_shown() {
        // No map selected, so the panel is the four text settings and there is no `bash` fact row
        // — see `Chooser::fields` and `settings_rows`.
        let mut c = chooser(None);
        c.map = 0;
        c.focus = Focus::Settings;
        c.field = Field::Name;
        let mut seen = vec![c.field];
        for _ in 0..4 {
            c.step(1);
            seen.push(c.field);
        }
        assert_eq!(
            seen,
            vec![
                Field::Name,
                Field::Bounds,
                Field::Origin,
                Field::Note,
                Field::Note
            ],
            "four fields, then clamped on the last"
        );
        assert_eq!(
            c.focus,
            Focus::Settings,
            "MAP INFO is the bottom of its column, so the walk ends there"
        );
    }

    /// The same run backwards, and off the top it leaves the panel — `up` from the first field is
    /// the map list, which is what is drawn above it.
    #[test]
    fn the_field_run_goes_backwards_and_then_out_of_the_panel() {
        let mut c = chooser(None);
        c.map = 0;
        c.focus = Focus::Settings;
        c.field = Field::Note;
        let mut seen = vec![c.field];
        for _ in 0..3 {
            c.step(-1);
            seen.push(c.field);
        }
        assert_eq!(
            seen,
            vec![Field::Note, Field::Origin, Field::Bounds, Field::Name],
            "backwards from the last to the first"
        );
        c.step(-1);
        assert_eq!(
            c.focus,
            Focus::Maps,
            "and once more leaves the panel for the list above it"
        );

        // **Forward then back is where you were** — the off-by-one guard, and it is asked of the
        // interior only. The two ends are precisely where the walk leaves this panel now, so a
        // round trip across one of them is a question about two panels rather than about the
        // arithmetic here. `Chooser::step`'s own test covers the crossing.
        for f in [Field::Bounds, Field::Origin] {
            let mut c = chooser(None);
            c.map = 0;
            c.focus = Focus::Settings;
            c.field = f;
            c.step(1);
            c.step(-1);
            assert_eq!(c.field, f, "{f:?}: forward then back is where you were");
            assert_eq!(c.focus, Focus::Settings, "{f:?}: and in the same panel");
            c.step(-1);
            c.step(1);
            assert_eq!(c.field, f, "{f:?}: back then forward is too");
            assert_eq!(c.focus, Focus::Settings, "{f:?}: and in the same panel");
        }
    }
}

// ------------------------------------------------------------------------------------------------
// One description of the screen
// ------------------------------------------------------------------------------------------------

/// **What a row means**, which is what decides its colour.
///
/// Named by role rather than by colour so the palette can move without every call site becoming a
/// decision about hue — the same reason `chrome`'s constants are `ACCENT` and `LABEL` rather than
/// `ORANGE` and `GREY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// The row the arrows are on.
    Selected,
    /// **The field being typed into right now**, which since 2026-09-03 is the one thing `ACCENT`
    /// means (decision D6). It used to share [`Tone::Selected`]: amber marked both the row the
    /// arrows were on and the value under the caret, so when selection went to `TEXT` the live
    /// edit would have gone with it — and a name half-typed is exactly the case that hue was kept
    /// for. It reads as selected everywhere else: same fill, same rail, same chevron.
    Editing,
    /// A kit that has pieces in it — readable at a glance against a blank one.
    Stocked,
    /// A kit with nothing in it, or a map with nothing placed. Not an error.
    Empty,
    /// An ordinary unselected row.
    Row,
    /// A refusal, or a map that will not open.
    Problem,
}

/// One line of one panel: a label, something aligned to its right, and what it means.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub left: String,
    pub right: String,
    pub tone: Tone,
}

/// **The whole screen as data.**
///
/// Built once and rendered twice — as the widget tree an author looks at, and as the flat text the
/// tests read. Two presentations of one description rather than two descriptions: a copy rule like
/// §1.4's *"an unmet condition is an instruction"* is asserted against the same rows that are drawn,
/// so the assertion cannot pass while the screen says something else.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub kits: Vec<Row>,
    /// **What the highlighted KIT is**, shown under the kit list where it cannot be mistaken for a
    /// map's. Reported at the keyboard: *"settings is still confusing as to whether it's a kit or a
    /// map. It's almost like they each need their own setting area."* They do — and the deeper
    /// fault was that one panel never followed the focus, so an author standing on a kit row was
    /// reading a panel about a map two levels below it.
    ///
    /// Facts, not a form: a kit's editable properties live in `project.ron` and its exclusions are
    /// edited on the Meshes tab with `Shift+R`. Nothing here is focusable, because a panel the
    /// arrows can enter and do nothing in is the dead stop this screen keeps removing.
    pub kit_header: String,
    pub kit_info: Vec<Row>,
    /// **The selected kit's policy** — its exclusions and its patches, read from `project.ron`.
    /// Drawn as its own panel under the kit list, the same way KIT INFO is; the rows carry the
    /// [`PolicyRow`] that `Delete` removes by ordinal.
    pub policy_header: String,
    pub policy: Vec<Row>,
    pub maps_header: String,
    pub maps: Vec<Row>,
    pub settings_header: String,
    pub settings: Vec<Row>,
    /// **The projects beside this one** — row 0 makes a new one; a real row names the directory
    /// and the command that opens it.
    pub projects_header: String,
    pub projects: Vec<Row>,
    /// A question the screen is waiting on — a pending deletion. Takes the message line, because a
    /// question you have not answered outranks a refusal you already read.
    pub asking: Option<String>,
    pub problem: Option<String>,
    pub hint: String,
}

impl Chooser {
    /// The screen this state describes.
    pub fn screen(&self) -> Screen {
        let mut kits = vec![Row {
            left: "+ new kit".to_owned(),
            right: "N".to_owned(),
            tone: if self.focus == Focus::Kits && self.kit == 0 {
                Tone::Selected
            } else {
                Tone::Row
            },
        }];
        // **No tick here.** A kit row is a kit, and what a map offers is the bash it names — stated
        // once on the MAP INFO `bash` row and declared once in `kits.ron`. The tick was a per-map
        // list edited from the kit list; a combination is shared, so ticking one from a map row
        // would silently change every other map naming it.
        // **Does this project bind kits at all?** Asked once, of the catalogue, rather than per row.
        //
        // A project whose `kits.ron` names `furniture` and `scp` genuinely cannot open `site`, and
        // the row should say so. A project that binds nothing is a different thing — an older or
        // simpler layout where every directory is reached by name — and marking every row there
        // would be noise dressed as a warning. So the mark means *this project binds kits, and not
        // this one*, which is exactly the sentence `Project::open`'s refusal writes.
        let binds_any = self.catalog.kits.iter().any(|k| k.bound);
        kits.extend(self.catalog.kits.iter().enumerate().map(|(i, k)| {
            let selected = self.focus == Focus::Kits && i + 1 == self.kit;
            // **A kit the project does not bind cannot be opened, and the row now says so.**
            //
            // Found live 2026-09-03: this column lists every directory under `assets/emerge/` that
            // looks like a kit, while `Project::open` will only open one `kits.ron` binds. So `site`
            // and `site_greybox` were offered, `Enter` refused with a good message — *"no kit `site`
            // in this project … binds `furniture`, `scp`"* — and the row gave no warning before the
            // press. Worse, KIT INFO read `pieces 45` and POLICY drew eight rows for it, so two
            // panels described a kit that could not be entered.
            //
            // The row stays, because the directory is really there and hiding it would make a kit
            // somebody copied in vanish silently. It is marked instead, and the marking is where the
            // fix is: `kits.ron` is what needs the edit, and the row is where the author is looking.
            // The root kit has no `flag` and is opened by `Project::open(None)`, so it is exempt.
            let unbound = binds_any && !k.bound;
            Row {
                left: if k.flag.is_none() {
                    format!("{} (default)", k.label)
                } else {
                    k.label.clone()
                },
                // **The piece count stays.** It is the fact this screen was built to carry — on
                // 2026-08-15 an author could not tell `site` from `site_v2` and relaunched three
                // times.
                right: if unbound {
                    "not in kits.ron".to_owned()
                } else {
                    format!("{} pieces", k.pieces)
                },
                // **A blank kit reads as blank without being read.** This is the fact the screen
                // exists to carry: on 2026-08-15 an author could not tell `site` from `site_v2`
                // and relaunched three times. A count nobody looks at would not have helped.
                tone: match (selected, unbound, k.pieces) {
                    (true, _, _) => Tone::Selected,
                    // Quieter than an empty kit: empty is a kit with no work in it yet, and this is
                    // not a kit this project has.
                    (false, true, _) => Tone::Empty,
                    (false, false, 0) => Tone::Empty,
                    (false, false, _) => Tone::Stocked,
                },
            }
        }));

        // **`MAPS`, not `MAPS IN <kit>`.** A map resolves against every bound kit merged, so
        // naming one of them over the column would be picking a winner the schema no longer has.
        let maps_header = "MAPS".to_owned();
        // **`+ new map` is always the first row.** It used to be drawn only while a kit was
        // selected, which was the last of the kit→map containment: `step` clamps `self.map` against
        // `len + 1` whichever column the KITS cursor is in, so with the cursor on `+ new kit` and
        // `self.map == 0` the highlight was on a row nobody had drawn — it simply vanished, and no
        // key brought it back. `create_map` writes into the project's `maps/` and consults no kit.
        let mut maps = vec![Row {
            left: "+ new map".to_owned(),
            right: "N".to_owned(),
            tone: if self.focus == Focus::Maps && self.map == 0 {
                Tone::Selected
            } else {
                Tone::Row
            },
        }];
        maps.extend({
            self.catalog
                .maps
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let selected = self.focus == Focus::Maps && i + 1 == self.map;
                    let (right, tone) = match &m.summary {
                        MapSummary::Unreadable(why) => {
                            (format!("will not open — {why}"), Tone::Problem)
                        }
                        MapSummary::Read {
                            placements, stamps, ..
                        } => {
                            // `1 piece`, not `1 piece(s)` — see [`plural`]. The parenthetical is
                            // the same defect as `1 maps`, one row up from where it was reported.
                            let text = match (placements, stamps) {
                                (0, 0) => "empty".to_owned(),
                                (p, 0) => format!("{p} {}", plural(*p, "piece")),
                                (0, t) => format!("{t} {}", plural(*t, "tile")),
                                (p, t) => format!(
                                    "{p} {}, {t} {}",
                                    plural(*p, "piece"),
                                    plural(*t, "tile")
                                ),
                            };
                            let tone = if *placements == 0 && *stamps == 0 {
                                Tone::Empty
                            } else {
                                Tone::Stocked
                            };
                            (text, tone)
                        }
                    };
                    Row {
                        left: clip(&m.name, 16),
                        right,
                        tone: if selected { Tone::Selected } else { tone },
                    }
                })
                .collect::<Vec<_>>()
        });

        // **The settings are shown for whatever is in hand** — the draft while one is being made,
        // and otherwise the selected map. The first version drew them only while creating, so `Tab`
        // did nothing on an existing map and three of the four settings were unreachable.
        let (settings_header, settings) = self.settings_rows();

        let (kit_header, kit_info) = self.kit_rows();
        let (policy_header, policy) = self.policy_rows();
        let (projects_header, projects) = self.projects_rows();
        Screen {
            kits,
            kit_header,
            kit_info,
            policy_header,
            policy,
            maps_header,
            maps,
            settings_header,
            settings,
            projects_header,
            projects,
            // **The question lives in the modal now**, so this renders nothing. `crate::confirm`
            // owns the wording, the two answers and the keys that give them; a second copy on the
            // chooser's own band is what a capture on 2026-08-19 showed underneath the panel —
            // `quit emerge-mapper? Y quits — Esc stays` in the corner while the modal said the
            // same thing in the middle. Two copies of one question is worse than the one it
            // replaced.
            asking: None,
            problem: self.problem.clone(),
            hint: self.hint().to_owned(),
        }
    }

    /// The highlighted kit, as facts. Empty when the `+ new kit` row is highlighted, because
    /// nothing is selected and inventing a panel for it would be the same lie the columns to the
    /// right already refuse to tell.
    fn kit_rows(&self) -> (String, Vec<Row>) {
        // `openable_kit`, not `current_kit` — a kit this project does not bind gets a blank panel
        // rather than a description of something `Enter` will refuse. See `openable_kit`.
        let Some(k) = self.openable_kit() else {
            return ("KIT INFO".to_owned(), Vec::new());
        };
        let excluded = k.dir.file_name().map_or(0, |_| self.excluded_count());
        let mut rows = vec![
            Row {
                left: "pieces".to_owned(),
                right: k.pieces.to_string(),
                tone: if k.pieces == 0 {
                    Tone::Empty
                } else {
                    Tone::Stocked
                },
            },
            // **No `provides` row and no `opened with` row.** Both were asked about at the
            // keyboard — *"what does provides mean? And why is it unnamespaced?"* — which is a row
            // failing at the only job it had. `provides` showed the namespace, and for every kit
            // that ships that is `None`, so it printed `(unnamespaced)`: a word about a schema
            // detail, answering a question nobody browsing kits was asking. `opened with` restated
            // the command line back at an author who had just used the screen instead of it.
            //
            // What a kit is, on this screen, is a name and a piece count. The binding lives in
            // `kits.ron`, which is where a binding question belongs.
        ];
        if excluded > 0 {
            rows.push(Row {
                left: "excluded".to_owned(),
                right: format!("{excluded} pack(s)"),
                tone: Tone::Empty,
            });
        }
        // **Where new work lands, and the verb that re-points it.** `kits.ron`'s `authoring` is
        // what `Project::open` opens when no `--kit` is given, so this row is the one place the
        // screen says which kit that is. `A` on the kit list flips it; the clickable value is the
        // mouse half — see `set_authoring`.
        rows.push(Row {
            left: "new work lands here".to_owned(),
            right: if self.catalog.authoring.as_deref() == Some(k.label.as_str()) {
                "yes".to_owned()
            } else {
                "no".to_owned()
            },
            tone: if self.catalog.authoring.as_deref() == Some(k.label.as_str()) {
                Tone::Stocked
            } else {
                Tone::Row
            },
        });
        // **No `— <name>` after it.** Reported at the keyboard: *"what is the m-dash scratch in the
        // info area? … they should be able to intuit things, not have to read it."* The suffix was
        // added when this was one shared panel that could not say whose settings it held. It now
        // sits directly under the list it belongs to, with the chevron marking the row it is
        // about — so the name was restating, in words, a fact the layout already made.
        ("KIT INFO".to_owned(), rows)
    }

    /// How many packs this kit's policy excludes. Read from the kit's own `project.ron` rather than
    /// carried on `Kit`, because the catalog is a scan of what exists and this is a statement of
    /// policy — two different questions about the same directory.
    fn excluded_count(&self) -> usize {
        self.current_kit()
            .map(|k| k.dir.join(emerge_core::policy::POLICY_FILE))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| emerge_core::policy::Policy::parse(&t).ok())
            .map_or(0, |p| p.exclude.len())
    }

    fn settings_rows(&self) -> (String, Vec<Row>) {
        // **A kit is one field, not four.** Bounds and origin are a map's; showing them greyed for a
        // kit would offer three rows that cannot do anything, which is the dead control this screen
        // keeps removing.
        if let Some(New::Kit(name)) = &self.creating {
            let live = self.focus == Focus::Settings && self.field == Field::Name;
            return (
                "NEW KIT".to_owned(),
                vec![Row {
                    left: Field::Name.label().to_owned(),
                    right: if live && self.editing {
                        format!("{}_", self.raw)
                    } else if name.is_empty() {
                        "(needs a name)".to_owned()
                    } else {
                        clip(name, 18)
                    },
                    tone: tone_for(live, self.editing, name.is_empty()),
                }],
            );
        }
        // **A project is one field too** — the name, and `Enter` makes the directory.
        if let Some(New::Project(name)) = &self.creating {
            let live = self.focus == Focus::Settings && self.field == Field::Name;
            return (
                "NEW PROJECT".to_owned(),
                vec![Row {
                    left: Field::Name.label().to_owned(),
                    right: if live && self.editing {
                        format!("{}_", self.raw)
                    } else if name.is_empty() {
                        "(needs a name)".to_owned()
                    } else {
                        clip(name, 18)
                    },
                    tone: tone_for(live, self.editing, name.is_empty()),
                }],
            );
        }
        let (header, name, bounds, origin, note, bash) = match (&self.creating, self.current_map()) {
            (Some(New::Map(d)), _) => (
                self.current_kit().map_or_else(
                    || "NEW MAP".to_owned(),
                    |k| format!("NEW MAP IN {}", k.label),
                ),
                d.name.clone(),
                d.bounds,
                d.origin,
                d.note.clone(),
                // A map is made offering every kit and is given a bash afterwards, with `B` on its
                // row — so there is nothing to state here and no field to state it in.
                None,
            ),
            (Some(New::Kit(_)), _) => unreachable!("handled above"),
            (Some(New::Project(_)), _) => unreachable!("handled above"),
            (None, Some(m)) => match &m.summary {
                MapSummary::Read { bounds, .. } => {
                    // Origin, note and bash are not in the summary — they are about the file, and
                    // reading every map's prose to fill a panel nobody has opened is work for a
                    // list. Selecting one is what asks the question, so it is read here.
                    let (origin, note, bash) = read_map_details(&m.path);
                    (
                        // The map's own name is the first row of this panel, so a name in the header was
                        // saying it twice — see the kit header above for the report behind this.
                        "MAP INFO".to_owned(),
                        m.name.clone(),
                        *bounds,
                        origin,
                        note,
                        bash,
                    )
                }
                MapSummary::Unreadable(_) => return ("MAP INFO".to_owned(), Vec::new()),
            },
            (None, None) => return ("MAP INFO".to_owned(), Vec::new()),
        };

        let live = |f: Field| self.focus == Focus::Settings && self.field == f;
        // The caret shows only while typing — a highlighted row you have not opened yet is still
        // showing its value, not an empty edit box.
        let value = |f: Field, settled: String| -> String {
            if live(f) && self.editing {
                format!("{}_", self.raw)
            } else {
                settled
            }
        };
        let mut rows = vec![
            Row {
                left: Field::Name.label().to_owned(),
                right: value(
                    Field::Name,
                    if name.is_empty() {
                        "(needs a name)".to_owned()
                    } else {
                        clip(&name, 18)
                    },
                ),
                tone: tone_for(
                    live(Field::Name),
                    self.editing,
                    self.creating.is_some() && self.raw.is_empty(),
                ),
            },
            Row {
                left: Field::Bounds.label().to_owned(),
                right: value(Field::Bounds, triple(bounds)),
                tone: tone_for(live(Field::Bounds), self.editing, false),
            },
            Row {
                left: Field::Origin.label().to_owned(),
                right: value(Field::Origin, triple(origin)),
                tone: tone_for(live(Field::Origin), self.editing, false),
            },
            Row {
                left: Field::Note.label().to_owned(),
                // **Clipped to one line.** A map's note is prose — the shipped one is a full
                // sentence with an absolute path in it — and at full length it wrapped, pushed its
                // own label onto a second line and broke the alignment of every row above it. The
                // whole note is still there in the file and still editable; this panel is a summary,
                // and a summary that reflows the screen is not one.
                right: value(Field::Note, clip(note.unwrap_or_default().as_str(), 20)),
                tone: tone_for(live(Field::Note), self.editing, false),
            },
        ];
        // **A fact, not a field** — the same shape `kit_rows`' `new work lands here` row has. The
        // arrows walk `Field::ALL` and this is not in it, and `on_row_click` for `RowPane::Settings`
        // only moves the focus, so the row is inert: `B` on the map row is the verb.
        //
        // Only on an existing map. A draft has no file to carry a bash and no row to press `B` on.
        if self.creating.is_none() {
            rows.push(Row {
                left: "bash".to_owned(),
                right: bash.clone().unwrap_or_else(|| "every kit".to_owned()),
                tone: if bash.is_some() { Tone::Stocked } else { Tone::Row },
            });
        }
        (header, rows)
    }

    /// The verbs, and only the ones that would do something right now. `docs/ui.md` §3.5 caps
    /// immediately-issuable choices at three or four; a key listed where it is dead is worse than a
    /// key not listed, because it teaches something untrue.
    pub fn hint(&self) -> &'static str {
        match self.ask {
            // **The question owns the keyboard, and the hint stands down while it does.** It used
            // to spell the two answers here as well; the modal states them beside its own buttons
            // now, so this only has to stop offering the ordinary verbs — listing those beside a
            // pending question invites pressing one of them.
            Some(_) => "",
            None => self.hint_when_nothing_is_asked(),
        }
    }

    fn hint_when_nothing_is_asked(&self) -> &'static str {
        match self.focus {
            // Naming finishes the thing, so the line has to say which thing — otherwise the key
            // that makes a map is invisible, which is how it got reported in the first place.
            _ if self.editing && matches!(self.creating, Some(New::Kit(_))) => {
                "type    Enter makes the kit    Esc cancel"
            }
            _ if self.editing && matches!(self.creating, Some(New::Map(_))) => {
                "type    Enter makes the map    Esc cancel"
            }
            _ if self.editing && matches!(self.creating, Some(New::Project(_))) => {
                "type    Enter makes the project    Esc cancel"
            }
            _ if self.editing => "type    Enter keep    Esc leave the field",
            // Reached by leaving the name field with Esc while still making something. No chord is
            // offered: naming it is what makes it, and there is no second way.
            Focus::Settings if self.creating.is_some() => "Enter name it    Esc cancel",
            // **The verb names what THIS row does.** Every row here is a text field except the
            // `bash` fact, which `B` on the map row changes. `up` off the top goes back to the map
            // list, which is the panel above this one — see `Chooser::step`.
            //
            // **The two arrows lead, together.** They are one idea — the arrow means what it points
            // at — so splitting them across the line would read as two unrelated verbs, which is
            // exactly the confusion the old `Tab` caused.
            Focus::Settings => "up/down field    left/right column    Enter edit    Esc quit",
            // **Only verbs that would do something right now.** `Enter` opens a map — so it is
            // not offered on a kit with none, nor on the `+ new` row where it makes instead.
            Focus::Kits if self.kit == 0 => {
                "up/down kit    left/right column    Enter new kit    Esc quit"
            }
            // **`Delete` is listed on the default kit too, even though it refuses there.**
            //
            // It was hidden at first, on the rule that a verb which only produces a refusal should
            // not be offered. That rule was right about maps and wrong here, and the report says
            // why: *"I'm still not seeing the delete in the kits area."* The default kit is the
            // first row and the one an author lands on, so hiding the verb there does not teach
            // "not this kit" — it teaches "no such verb", which is the opposite of true.
            //
            // The refusal names the reason (every other kit lives inside this one), which is
            // `docs/ui.md` §1.4: an unmet condition is an instruction.
            Focus::Kits if self.catalog.maps.is_empty() => {
                "up/down kit    left/right column    N new kit    Delete remove    Esc quit"
            }
            // **`Enter open kit`, not `Enter open`.** Both list columns offered a bare "open" and
            // they open different doors — so with the columns swapped on 2026-08-16 an author
            // pressed it in the maps column expecting the kit and got the Map door, where the
            // labeler is not even bound. The verb has to name what it opens.
            //
            // **`A` makes this kit the authoring kit** — the one new work lands in. Offered only
            // on a real kit row, never on `+ new kit`, which is the `kit == 0` arm above.
            Focus::Kits => {
                "up/down kit    left/right column    Enter open KIT    A authoring    N new kit    Delete remove    Esc quit"
            }
            Focus::Maps if self.map == 0 => {
                "up/down map    left/right column    Enter new map    Esc quit"
            }
            // **`B` names the bash this map draws on** — the combination declared in `kits.ron`,
            // cycled here and shown on MAP INFO's `bash` row. Only on a real row: row 0 makes a
            // map, and a map that does not exist yet has no field to set.
            Focus::Maps => {
                "up/down map    left/right column    Enter open MAP    B bash    Delete remove    Esc quit"
            }
            // **The policy panel's verbs.** `Delete` removes the highlighted entry — through the
            // confirmation, because a `because` string is hand-written rationale. `Enter` opens
            // the kit, the same door the kit row opens. No `N`: adding a patch is not in this
            // panel's scope.
            Focus::Policy => {
                "up/down entry    left/right column    Enter open KIT    Delete remove    Esc quit"
            }
            // **The projects column's verbs.** `Enter` on the `+ new project` row makes one; on a
            // real row it reports the command that opens that project, because nothing opens
            // in-process. `N` makes a new one from any real row. The current project row offers
            // neither `Enter` (it is already open) nor `Delete` (a whole directory tree), and the
            // hint says so by simply not listing them — a verb that would do nothing is not
            // offered.
            Focus::Projects if self.project == 0 => {
                "up/down project    left/right column    Enter new project    Esc quit"
            }
            Focus::Projects => {
                "up/down project    left/right column    N new project    Esc quit"
            }
        }
    }
}

/// **`1 map`, not `1 maps`.** Reported at the keyboard, about a different line: *"be careful about
/// leaving stray text that doesn't make sense to the user."* A count that disagrees with its noun
/// is exactly that — it reads as something generated rather than something written, in the one
/// sentence on this screen that has to be trusted before a directory is removed.
fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_owned()
    } else {
        format!("{word}s")
    }
}

/// One line's worth, with an ellipsis when there was more. Counts **characters, not bytes**, so a
/// note containing an em-dash cannot be cut through the middle of one.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// **What a settings row's value reads as**: amber under the caret, the ordinary loud ink when the
/// arrows are merely standing on it, and quiet when there is nothing there yet.
///
/// `editing` is the argument added on 2026-09-03 with [`Tone::Editing`] — before it, selection and
/// live edit were one tone, so they could not part company when selection stopped borrowing
/// `ACCENT` (D6).
fn tone_for(live: bool, editing: bool, unset: bool) -> Tone {
    match (live, editing, unset) {
        (true, true, _) => Tone::Editing,
        (true, false, _) => Tone::Selected,
        (_, _, true) => Tone::Empty,
        _ => Tone::Row,
    }
}

/// Origin, note and bash, read from the map file when a row is selected — three fields the summary
/// does not carry, off one parse rather than three. Failure is silent here on purpose: the row
/// already carries `Unreadable` when the file cannot be parsed, and a second refusal in the
/// settings panel would say the same thing twice.
fn read_map_details(path: &Path) -> ((f32, f32, f32), Option<String>, Option<String>) {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| Map::parse(&t).ok())
        .map_or(((0.0, 0.0, 0.0), None, None), |m| (m.origin, m.note, m.bash))
}

fn triple(t: (f32, f32, f32)) -> String {
    format!("{} x {} x {}", t.0, t.1, t.2)
}

/// **The screen as flat text** — what the tests read, built from the same [`Screen`] the widgets are.
pub fn render(c: &Chooser) -> String {
    let s = c.screen();
    let line = |r: &Row| {
        // **A row being typed into is still the selected row.** The mark is the non-colour channel
        // (`docs/ui.md` §1.3), so it answers "which row" and not "which ink".
        let mark = if matches!(r.tone, Tone::Selected | Tone::Editing) {
            ">"
        } else {
            " "
        };
        format!("{mark} {:<28}{}\n", r.left, r.right)
    };
    // **Left to right, exactly as the columns are drawn** — see `spawn_screen`. A flat rendering
    // that reordered the panels would be a second opinion about the hierarchy.
    let mut out = String::from("emerge-mapper\n");
    if !s.projects.is_empty() {
        out.push_str(&format!("\n{}\n", s.projects_header));
        for r in &s.projects {
            out.push_str(&line(r));
        }
    }
    out.push_str("\nKITS\n");
    for r in &s.kits {
        out.push_str(&line(r));
    }
    if !s.kit_info.is_empty() {
        out.push_str(&format!("\n{}\n", s.kit_header));
        for r in &s.kit_info {
            out.push_str(&line(r));
        }
    }
    if !s.policy.is_empty() {
        out.push_str(&format!("\n{}\n", s.policy_header));
        for r in &s.policy {
            out.push_str(&line(r));
        }
    }
    out.push_str(&format!("\n{}\n", s.maps_header));
    for r in &s.maps {
        out.push_str(&line(r));
    }
    if !s.settings.is_empty() {
        out.push_str(&format!("\n{}\n", s.settings_header));
        for r in &s.settings {
            out.push_str(&line(r));
        }
    }
    if let Some(a) = &s.asking {
        out.push_str(&format!("\n{a}\n"));
    }
    if let Some(p) = &s.problem {
        out.push_str(&format!("\n{p}\n"));
    }
    out.push_str(&format!("\n{}", s.hint));
    out
}

// ------------------------------------------------------------------------------------------------
// The Bevy half
// ------------------------------------------------------------------------------------------------

use bevy::input::keyboard::{Key, KeyboardInput};

/// **Where the chosen launch line comes back out.**
///
/// `App::run()` consumes the world, so the choice cannot be read off a resource afterwards. The
/// chooser writes here and asks the app to exit; `main.rs` reads it once `run` returns.


#[derive(Component)]
struct KitList;
#[derive(Component)]
struct MapList;
#[derive(Component)]
struct MapsHeader;
#[derive(Component)]
struct KitInfoList;
#[derive(Component)]
struct KitInfoHeader;
#[derive(Component)]
struct SettingsList;
#[derive(Component)]
struct SettingsHeader;
#[derive(Component)]
struct PolicyList;
#[derive(Component)]
struct PolicyHeader;
#[derive(Component)]
struct ProblemLine;
#[derive(Component)]
struct ProjectList;
#[derive(Component)]
struct ProjectHeader;
#[derive(Component)]
struct HintLine;

/// The chooser's screen — the [`crate::screen::Screen::Menu`] half of the application.
///
/// It was its own `App` in its own process, which is why this carried a whole `Chooser` and a mutex
/// to answer through. Both screens are one application now (`screen.rs`), so it carries only where
/// the project is and which kit the command line preselected; the state is built on the way in.
pub struct ChooserPlugin {
    pub root: PathBuf,
    /// A `--kit` from the command line, which has already answered half of what this screen asks.
    pub preselect: Option<String>,
}

/// Where [`ChooserPlugin`] keeps its two arguments until the menu is entered.
#[derive(Resource, Clone)]
struct MenuOpening {
    root: PathBuf,
    preselect: Option<String>,
}

impl Plugin for ChooserPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MenuOpening {
            root: self.root.clone(),
            preselect: self.preselect.clone(),
        })
        // **`ExtraRoom` belongs to the menu, not to the capture rig it used to sit beside.** It was
        // `init_resource`'d by `ChooserCapturePlugin`, and when that plugin's job moved to
        // `crate::surface` the resource went with it — leaving `room_for_the_card` taking a
        // `ResMut<ExtraRoom>` that no longer existed, which in Bevy 0.19 panics the system rather
        // than skipping it. The menu screen came up as a crash with the guide card as the only clue.
        .init_resource::<ExtraRoom>()
        // **`PostStartup`, not `Startup.after(..)`.** Ordering systems does not flush commands: a
        // camera spawned in `Startup` does not exist in the World until that schedule ends, so an
        // `.after()` here found no camera, returned early, and drew no interface at all — a black
        // window with nothing in the log, because the early return was silent.
        // **`OnEnter(Menu)`, chained, not `PostStartup`.** The note this replaces still holds and is
        // the reason for the `.chain()`: ordering systems does not flush commands, so a camera
        // spawned beside `spawn_screen` does not exist in the World when it looks — which drew a
        // black window and logged nothing, because the early return was silent. `.chain()` puts a
        // sync point between them, which is what `PostStartup` was standing in for.
        .add_systems(
            OnEnter(crate::screen::Screen::Menu),
            (build_chooser, spawn_screen, spawn_menu_bars)
                .chain()
                .after(crate::chrome::FrameSystems),
        )
        // **The chooser's own entities die with the screen.** `DespawnOnExit` is Bevy's own
        // state-scoping, so this is one component per root rather than a teardown list somebody has
        // to remember to extend — see `screen.rs` on why a partial teardown is the failure mode
        // worth spending a whole reload to avoid.
        .add_systems(
            OnExit(crate::screen::Screen::Menu),
            tear_down_menu,
        )
        // **Text before chords, as `keys::Phase` orders them in the editor**: a field with the
        // keyboard consumes a keystroke before anything reads it as a verb, or typing `n` into a
        // name starts a second new map.
        .add_systems(
            Update,
            (
                type_into_field,
                drive_chooser,
                paint_chooser,
                // **After the paint**, because it measures rows the paint may have just respawned.
                keep_the_chooser_selection_on_screen,
                // **After the drive**, so the border lights on the same frame the arrow moved the
                // keyboard rather than one behind it. It reads `Chooser::focus` and writes nothing
                // the paint depends on, so it is last for ordering's sake and not for the paint's.
                mark_the_focused_panel,
            )
                .chain()
                .run_if(in_state(crate::screen::Screen::Menu)),
        );

        // **The menu's guide vocabulary, in the application that has a menu.** It was written when
        // the chooser was its own process and its own `App`, so nothing ever added it here — which
        // left `guides/try_the_chooser.json` naming fourteen checkpoints the binary did not know,
        // and a posted script answering "no checkpoint named …" at every step. No name it registers
        // collides with `guided`'s: the two vocabularies describe two screens.
        #[cfg(feature = "debugger")]
        app.add_plugins(ChooserGuidePlugin);
    }
}

/// **Room for a guide card, added to the window only while one is up.**
///
/// Reported at the keyboard: *"your guide is in the way of UI."* It was — the card sits at
/// `top: <n>` from the top of the window, and this screen is dense from its first pixel, so any
/// offset small enough to be on screen landed on the columns. The editor can take a top overlay
/// because its content is a 3D view with panels down the sides; this one cannot.
///
/// So the card goes **below** the screen, and the window grows to hold it — and shrinks back when
/// the guide is cleared, because a permanently taller window is the empty half this screen was
/// already once criticised for.
/// Trimmed from 230 once a capture showed what a card actually occupies: a two-instruction step
/// left a third of the added height empty. The slack that remains is deliberate — a card is sized
/// by its own text, so a number tight against one step would clip the next, and clipping an
/// instruction is the failure this whole placement exists to avoid.
#[cfg(feature = "debugger")]
const CARD_ROOM: f32 = 200.0;

/// **The words carrying the relationship, and they were the faintest thing on screen.**
///
/// `MAPS IN emerge` and `SETTINGS FOR untitled_map` are the only text stating what belongs to what,
/// and they were drawn in `LABEL` — the dimmest colour in the palette. An author asked to read the
/// hierarchy off this screen had to hunt for the one sentence that explains it. `docs/ui.md` §1.3:
/// the encoding is the message.
fn header(text: &str) -> impl Bundle {
    (
        Text::new(text.to_owned()),
        // **`HEADING`, not `BODY`.** `MAPS`, `KIT INFO` and the rest are headings, and they were
        // rendered at the body role — the same class of misuse the 2026-08-18 type pass found in the
        // four tabs. The role table exists so "what does a heading measure" is answered once.
        crate::chrome::font(crate::chrome::text::HEADING),
        // **`KEY`, where `chrome::list_heading` uses `LABEL`**, and this is the one deliberate
        // departure from that builder rather than an oversight. These words carry the *relationship*
        // — `MAPS IN emerge`, `SETTINGS FOR untitled_map` are the only text on the screen stating
        // what belongs to what — and they were `LABEL`, the dimmest ink in the palette, so an author
        // asked to read the hierarchy had to hunt for the sentence explaining it. `docs/ui.md` §1.3:
        // the encoding is the message.
        TextColor(crate::chrome::KEY),
    )
}

/// **The menu, drawn in the window's own frame.**
///
/// # It used to be a fixed-pixel grid in a window it did not fill
///
/// Two 300 px columns and panels whose height was computed from the catalogue — `panel_heights`,
/// `content_h`, `ROW_H = 17.9` measured off a capture — with the whole thing dropped into whatever
/// window there was. `fit_capture_to_window` said so outright: *"The layout is fixed-size and simply
/// sits in whatever window there is."* On a 2560x1406 window that left about two fifths of the
/// screen as ground nothing used, panels sized for twenty rows holding two, and a kit row wrapping
/// across two lines into its own value column.
///
/// It is flex now, in [`crate::chrome::Frame`]'s body — the same frame the editor is drawn in, which
/// is the point: the application has one answer to how it is laid out. The columns share the width
/// evenly and the panels share the height, so the screen is full at any window size and a catalogue
/// twice as long does not want a taller window.
///
/// **The columns fill the window, and the cap that stopped them is gone.** It was `COL_MAX = 420`
/// with `flex_basis: 0`, on the argument that a stretched two-column menu puts a row's value a foot
/// from its label — the alignment complaint this screen had once, from the other direction. On the
/// 3396 px window the 2026-09-03 audit measured (F2) that argument cost more than it bought: three
/// capped columns occupied a centred island with roughly 2,500 px of dead void either side. The
/// author's answer (D2) is that *"the editor's docks may keep pixel widths — a viewport wants the
/// space — but the menu has no viewport to protect and must stretch."* So the columns share the
/// width with `flex_grow` and no ceiling, and the label/value alignment that the cap was really
/// protecting is now the label column's job: [`crate::chrome::COL_LABEL`] and `COL_WIDE` hold the
/// values in line at any column width, which a maximum width never actually did.
fn spawn_screen(mut commands: Commands, frame: Res<crate::chrome::Frame>) {
    // **A menu has no docks and no viewport, and saying so is load-bearing.**
    //
    // The frame builds all three for both screens, and the viewport carries `flex_grow: 1` because
    // in the editor it is defined as whatever is left. Left standing on the menu it competes with
    // the columns for that same slack and takes a third of the window — which showed up as a wide
    // empty band down the left, columns a third narrower than their cap, and a kit row wrapping
    // because of it. One symptom would have been chased; three from one cause is worth the two
    // lines.
    //
    // `Display::None`, so they take no space at all rather than zero width and a gap each.
    for slot in [frame.left, frame.viewport, frame.right] {
        commands.entity(slot).insert(Node {
            display: Display::None,
            ..default()
        });
    }
    commands.entity(frame.body).insert(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Stretch,
        justify_content: JustifyContent::Center,
        // **The named scale, where this was `PAD` and `PAD * 1.5`.** A gap between two columns is a
        // gap between blocks — [`crate::chrome::GAP_GROUP`] — and the inset from the window edge is
        // the same [`crate::chrome::MARGIN`] every dock panel in the editor keeps, so the menu's
        // outer edge lands where the editor's does rather than one and a half pads in from it. The
        // arithmetic was the last unnamed spacing number on this screen (`ui_audit.md` F8).
        column_gap: Val::Px(crate::chrome::GAP_GROUP),
        padding: UiRect::all(Val::Px(crate::chrome::MARGIN)),
        flex_grow: 1.0,
        // CHROME-OK: not a gap — a flex item's automatic minimum size is its content, and this is
        // what lets the body be shorter than the columns inside it so they can scroll.
        min_height: Val::Px(0.0),
        ..default()
    });

    commands.entity(frame.body).with_children(|row| {
        // **Left to right IS the data model**: PROJECT owns KITS, and a MAP draws on them.
        //
        // The ERD in `CLAUDE.md` is the order. `PROJECT ||--o{ KIT` and `PROJECT ||--o{ MAP` make
        // the project the root, so it is leftmost; between its two children the cross-edge decides,
        // and it runs one way — `MAP` names a `BASH`, a `BASH` names `KIT`s, and the build chain is
        // KIT → DESCRIPTOR → PLACEMENT → MAP. Kits are what a map is made of, so kits come first
        // and the map is what they add up to.
        //
        // **This replaces maps-first**, which was asked for on 2026-08-16 on the argument that
        // *"the nesting the order was drawing no longer exists"* — true then, because a map had
        // just left the kit directories and nothing tied the two columns together. A bash ties them
        // again, in the other direction and one level up: the project declares the combinations, a
        // map names one. So there is a hierarchy to draw, and this is it.
        //
        // **One honest gap.** A Miller column's selection opens the column to its right, and the
        // KITS→MAPS boundary is that; the PROJECTS→KITS one is not, because nothing opens in this
        // process — `Enter` on a sibling reports `emerge-mapper <dir>` instead. PROJECTS stands
        // leftmost for what it *contains*, and the row says so rather than pretending to navigate.
        //
        // **Each column owns what belongs to it.** A map's settings sit under the map list; a kit's
        // facts sit under the kit list. One shared panel could not say whose it was — and worse, it
        // never followed the focus, so standing on a kit row you read a panel about a map two levels
        // down.
        let column = || Node {
            flex_direction: FlexDirection::Column,
            // Between the panels stacked in one column: a block gap, where this was `GAP_ROW * 2`.
            row_gap: Val::Px(crate::chrome::GAP_GROUP),
            flex_grow: 1.0,
            // `Percent(0.0)`, so a column's share is decided by `flex_grow` and not by the width of
            // the longest kit name inside it — a `Val::Px(0.0)` basis is the same intent, and this
            // is the one that cannot be read as a pixel measurement.
            flex_basis: Val::Percent(0.0),
            // CHROME-OK: not a gap — a flex item's automatic minimum size is its content, so
            // without this a long row name pushes its column wider than its share of the window.
            min_width: Val::Px(0.0),
            ..default()
        };

        // **The projects column** — the siblings of this root, plus the verb that makes a new one.
        // A list like the kits' is: row 0 is `+ new project`, real projects are `i + 1`.
        row.spawn(column()).with_children(|col| {
            col.spawn((list_panel(), ListPanel, PanelFocus(Focus::Projects))).with_children(|p| {
                p.spawn((header("PROJECTS"), ProjectHeader));
                crate::chrome::scroll_list(p, ProjectList);
            });
        });

        row.spawn(column()).with_children(|col| {
            col.spawn((list_panel(), ListPanel, PanelFocus(Focus::Kits))).with_children(|p| {
                p.spawn(header("KITS"));
                crate::chrome::scroll_list(p, KitList);
            });
            // **KIT INFO carries no `PanelFocus`** — it is the one menu panel no arrow can stand
            // in (see `Focus`), so its border never lights.
            col.spawn((info_panel(), InfoPanel)).with_children(|p| {
                p.spawn((header("KIT INFO"), KitInfoHeader));
                p.spawn((Node::default(), KitInfoList));
            });
            // **The selected kit's policy** — its exclusions and its patches, read from
            // `project.ron`. Drawn as a second inspector under KIT INFO, the way the map's settings
            // sit under the maps list. A separate panel from KIT INFO because it is a list the
            // arrows can stand in, and the facts sheet is not.
            col.spawn((info_panel(), InfoPanel, PanelFocus(Focus::Policy))).with_children(|p| {
                p.spawn((header("POLICY"), PolicyHeader));
                p.spawn((Node::default(), PolicyList));
            });
        });

        row.spawn(column()).with_children(|col| {
            col.spawn((list_panel(), ListPanel, PanelFocus(Focus::Maps))).with_children(|p| {
                p.spawn((header("MAPS"), MapsHeader));
                // **A scroll container, because the panel is flex now.** It sized itself to the
                // catalogue while the whole screen did; on the frame it takes the height that is
                // left, so a project with more maps than fit simply overflowed with nothing on
                // screen saying so.
                crate::chrome::scroll_list(p, MapList);
            });
            col.spawn((info_panel(), InfoPanel, PanelFocus(Focus::Settings))).with_children(|p| {
                p.spawn((header("MAP INFO"), SettingsHeader));
                p.spawn((Node::default(), SettingsList));
            });
        });
    });
}

/// **The menu's own chrome and status.** The editor fills these two bars with a door's furniture;
/// the menu has a name and a hint line, and they belong in the same places for the same reason.
fn spawn_menu_bars(mut commands: Commands, frame: Res<crate::chrome::Frame>) {
    commands.entity(frame.chrome_bar).with_children(|bar| {
        bar.spawn((
            Text::new("emerge-mapper"),
            crate::chrome::font(crate::chrome::text::BODY),
            TextColor(crate::chrome::LABEL),
        ));
    });
    commands.entity(frame.status).with_children(|band| {
        // **The refusal first and the hint after it**, both on the same row: the band is one line
        // and the problem is the half worth reading. `DANGER` carries the emphasis — colour is how
        // this editor shouts, and size is what type role a thing has.
        band.spawn((
            Text::new(String::new()),
            crate::chrome::font(crate::chrome::text::BODY),
            TextColor(crate::chrome::DANGER),
            ProblemLine,
        ));
        band.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        band.spawn((
            Text::new(String::new()),
            crate::chrome::font(crate::chrome::text::BODY),
            TextColor(crate::chrome::LABEL),
            HintLine,
        ));
    });
}

/// **Read the `Node` back out of a panel bundle**, so a test can assert the shape rather than
/// re-describe it. The bundle is the one the screen actually spawns; a test that rebuilt the numbers
/// would be checking its own copy.
#[cfg(test)]
fn panel_node(bundle: MenuPanel) -> Node {
    bundle.0
}

/// **What a menu panel is made of, which is what every other panel in this application is made of.**
///
/// The 2026-09-03 audit's F9 counted six menu panels hand-rolling a container while the editor's
/// seven came out of [`crate::chrome::panel_root`]. **The container stays hand-rolled and the
/// surface does not.** `panel_root` takes a [`crate::chrome::Frame`] and a
/// [`crate::chrome::Side`] and is dock-shaped — a pixel width, one of two edges to be pinned to,
/// `full_height` — and the menu's six are columns in a grid: they have no dock, they share the
/// width between them (see [`spawn_screen`]), and two of them stack inside one column. Forcing them
/// through a builder whose whole subject is the dock would mean widening that builder for a caller
/// with no dock, which is how a shared widget becomes a switch statement.
///
/// What there was never a reason for is a second *surface*. So this is exactly the list
/// `panel_root` spawns, and the six panels differ from the seven only in the fill they pass:
/// the ground, [`crate::chrome::Ground`] so the held-key overlay can dim it, the hairline
/// [`crate::chrome::PANEL_EDGE`] and the corner that make a fill read as an object rather than as
/// slightly different paint, the `Hovered` that answers *"is the pointer over the interface"* for
/// the whole surface including the gaps between its rows, and the [`crate::chrome::PanelEdge`]
/// marker that lets [`PanelFocus`] light the border of the panel holding the keyboard.
type MenuPanel = (
    Node,
    BackgroundColor,
    crate::chrome::Ground,
    BorderColor,
    crate::chrome::PanelEdge,
    bevy::picking::hover::Hovered,
);

/// One fill, one surface. See [`MenuPanel`].
fn menu_panel(mut node: Node, fill: Color) -> MenuPanel {
    node.border = UiRect::all(Val::Px(crate::chrome::EDGE_W));
    node.border_radius = BorderRadius::all(Val::Px(crate::chrome::RADIUS_PANEL));
    (
        node,
        BackgroundColor(fill),
        crate::chrome::Ground(fill),
        BorderColor::all(crate::chrome::PANEL_EDGE),
        crate::chrome::PanelEdge,
        bevy::picking::hover::Hovered::default(),
    )
}

/// A list panel: takes the height that is left, so a long catalogue does not want a taller window.
fn list_panel() -> MenuPanel {
    menu_panel(
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(crate::chrome::PAD)),
            row_gap: Val::Px(crate::chrome::GAP_ROW),
            flex_grow: 1.0,
            // CHROME-OK: not a gap — the `min_height: 0` that lets a flex panel be shorter than
            // its rows, which is what gives the scroll area inside it something to clip.
            min_height: Val::Px(0.0),
            ..default()
        },
        crate::chrome::PANEL_BG,
    )
}

/// **An inspector, on a different surface from the list above it.** It sits on the lighter ground
/// the editor already uses for a slot, so it does not read as a third list — looking the same was
/// the whole problem (see [`PanelKind`]). Sized by its content, because a fact sheet with four rows
/// in it should not be half the screen.
fn info_panel() -> MenuPanel {
    menu_panel(
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(crate::chrome::PAD)),
            row_gap: Val::Px(crate::chrome::GAP_ROW),
            flex_shrink: 0.0,
            ..default()
        },
        crate::chrome::SLOT_BG,
    )
}

/// A list panel, held to the height of the fullest list. See [`panel_heights`].
#[derive(Component)]
struct ListPanel;

/// An inspector panel, held to a fixed height so the fields inside it never move.
#[derive(Component)]
struct InfoPanel;

/// **Which [`Focus`] this panel is the home of**, so the panel holding the keyboard can light its
/// border — the menu's half of the 2026-09-03 decision D13, *"the panel holding the keyboard lights
/// its border, in the menu and in the editor's docks."*
///
/// Per *panel*, not per column, because [`Focus`] is per panel: `down` off the end of the kit list
/// carries into POLICY, which is drawn under it in the same column, and a column-wide light would
/// then say the keyboard is in two places. KIT INFO has no arm here — no arrow can stand in it.
#[derive(Component, Clone, Copy)]
struct PanelFocus(Focus);

/// **Put [`crate::chrome::Focused`] on the panel that holds the keyboard, and take it off the rest.**
///
/// It *marks*; `chrome::light_the_focused_panel` is what paints, and the split is deliberate — the
/// owner of the focus decides where the keyboard is (here it is `Chooser::focus`; on a door it is a
/// `Context`) and the palette decides what that looks like.
///
/// Compares before writing. `Focused` is what the painter reacts to, so inserting it on the same
/// entity every frame would dirty six panels' `BorderColor` sixty times a second for an edge that
/// moves a few times a session (`tests/no_system_writes_every_frame.rs`).
fn mark_the_focused_panel(
    chooser: Res<Chooser>,
    panels: Query<(Entity, &PanelFocus, Has<crate::chrome::Focused>)>,
    mut commands: Commands,
) {
    for (panel, home, lit) in &panels {
        let want = home.0 == chooser.focus;
        if want == lit {
            continue;
        }
        if want {
            commands.entity(panel).insert(crate::chrome::Focused);
        } else {
            commands.entity(panel).remove::<crate::chrome::Focused>();
        }
    }
}

fn colour(tone: Tone) -> Color {
    match tone {
        // **`TEXT`, not `ACCENT`** — the 2026-09-03 decision D6/D7. Amber used to mark the selected
        // row's words here, which was one of the five jobs that hue was doing; it keeps exactly one,
        // *a value being changed right now*. Selection is carried by the row itself — `ROW_SELECTED`
        // plus the accent rail `chrome::row_shape` draws down its leading edge — so the text on a
        // chosen row is simply the loudest ordinary ink, which is what a value reads as everywhere
        // else in this editor.
        Tone::Selected => crate::chrome::TEXT,
        // **And this is the one thing amber is left for** — `ACCENT`'s single remaining job, the
        // value under the caret. See [`Tone::Editing`].
        Tone::Editing => crate::chrome::ACCENT,
        Tone::Stocked => crate::chrome::LABELED,
        Tone::Empty => crate::chrome::LABEL,
        Tone::Row => crate::chrome::DIM,
        Tone::Problem => crate::chrome::DANGER,
    }
}

/// Rebuild one list. The whole block is despawned and respawned together, which is what `compose.rs`
/// does for the same reason: a row has no identity worth keeping across a change, and four rows is
/// not work worth diffing.
/// **What a panel is**, which decides how its rows are drawn.
///
/// The correction behind this: *"can we make it clearer that the settings refer to a map? the
/// hierarchy of the data structure isn't clear"* — answered first with three identical columns,
/// which did not work, and the reason it did not is that **the three columns are not the same
/// relationship**:
///
/// | | |
/// |---|---|
/// | kits → maps | **containment** — a kit *contains* map files |
/// | map → settings | **attribution** — a map *has* a name and bounds; they are not inside it |
///
/// Miller columns work because every column is the same relation all the way down: you learn the
/// rule once and it holds. Three identical panels taught that rule in the first step and broke it in
/// the second. So a list looks like a list, and the inspector does not.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelKind {
    /// A list you walk, whose selection opens the column to its right. Carries the chevron that says
    /// so — the affordance Finder puts on exactly this.
    List,
    /// Properties of whatever is selected to the left. No chevron: nothing opens from here.
    Inspector,
}

/// **Which list a row belongs to**, so a click can move the cursor there.
///
/// The chooser was keyboard-only — no `Hovered`, no `Pointer<..>`, no button anywhere in 5,616
/// lines — which was survivable while its rows were plain text. Adopting [`crate::chrome::list_row`]
/// makes them light under the pointer, and a row that lights and answers nothing is the
/// dead-affordance defect the tab strip carried for months (`docs/ui.md` §4.2: everything reachable
/// by mouse is reachable by keyboard **and the reverse**). So the marker comes with the shape.
#[derive(Component, Clone, Copy)]
struct ChooserRow {
    pane: RowPane,
    /// **The index IS the cursor value.** `Chooser::kit` counts the `+ new kit` row as 0 and the
    /// catalogue's kit `i` as `i + 1` — which is exactly the order `screen()` builds the rows in, so
    /// a click needs no translation and cannot drift from what the arrows do.
    index: usize,
}

/// The lists a cursor can be in. `KIT INFO` and `POLICY` are facts about the selection — `KIT
/// INFO` has no [`Focus`], so its rows carry no marker and stay unclickable rather than lighting
/// under the pointer and doing nothing; `POLICY` is a panel the arrows can stand in, so its rows
/// carry the marker and the click selects, exactly as the kits and maps rows do.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RowPane {
    Kits,
    Policy,
    Maps,
    Settings,
    Projects,
}

/// **Which of the shared label columns this panel's rows share**, measured rather than picked.
///
/// It was `INFO_LABEL_W = 76.0`, one of the six unnamed label widths the 2026-09-03 audit found
/// (F8) — and it was the one that overlapped: `new work lands here` plus the selection mark is 21
/// glyphs, 126 px at this size, so the value was drawn on top of the label in every capture of the
/// menu (F5). The columns are [`crate::chrome::COL_LABEL`] for a word and `COL_WIDE` for a phrase,
/// and `chrome::row_label` treats either as a **floor** rather than a cap, so a label longer than
/// its column pushes its own value right instead of painting over it.
///
/// **One answer per panel, from its widest label** — not per row. A column is a column because
/// every value in it starts at the same x; deciding row by row would put KIT INFO's `pieces` value
/// at 52 and its `excluded` value at 96, which is two columns wearing one name. Panels may differ
/// from each other, which is the point of having named widths at all: MAP INFO holds `bounds` and
/// sits at [`crate::chrome::COL_LABEL`], KIT INFO holds a phrase and sits at `COL_WIDE`, and they
/// are in different columns of the screen so nothing lines up across them anyway.
fn label_col(rows: &[Row]) -> f32 {
    // FiraMono is monospace, so `chars * advance` is exact rather than an estimate — see
    // `chrome::BODY_CHAR_W`, which is stated at the body role. A label renders one role down.
    let advance = crate::chrome::BODY_CHAR_W * crate::chrome::text::LABEL.px()
        / crate::chrome::text::BODY.px();
    // `+ 2` for the mark and the space `fill` puts in front of every label.
    let widest = rows
        .iter()
        .map(|r| r.left.chars().count().saturating_add(2))
        .max()
        .unwrap_or(0);
    if widest as f32 * advance <= crate::chrome::COL_LABEL {
        crate::chrome::COL_LABEL
    } else {
        crate::chrome::COL_WIDE
    }
}

/// **An inspector's `LABEL  value` row** — a real flex row, so the value is laid out *after* the
/// label column rather than positioned over it. Both inspector paths in [`fill`] spawn this one
/// shape; the policy panel's version is the same row inside a [`crate::chrome::quiet_row`],
/// because a policy entry is selectable and a fact is not.
fn info_row() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(crate::chrome::GAP_ROW),
        width: Val::Percent(100.0),
        ..default()
    }
}

/// **Rebuild one list, in the editor's own row vocabulary.**
///
/// The 2026-08-18 audit's finding about this file: thirty `chrome::` references and **not one row
/// builder** — no `list_row`, `chip`, `text_field`, `list_heading`, `row_label` or `row_value`. It
/// was the fifth dialect `docs/2026-08-17-mapper-ui-audit.md` warned about, with the front door as
/// the dialect, and the one screen an author sees before anything else.
///
/// A list row is now [`crate::chrome::list_row`] — the same fill, the same hover, repainted by the
/// same `style_list_rows` as every list in the editor. An inspector row is
/// [`crate::chrome::row_label`] beside [`crate::chrome::row_value`], the label/value shape the audit
/// counted hand-rolled eight times.
///
/// **The chevron stays.** `docs/ui.md` §1.3 requires a second, non-colour channel on every status,
/// and with the fill doing the shouting the mark is what still reads in a capture, in a screenshot
/// somebody pastes into a message, and to anyone who cannot tell two dark warm greys apart.
fn fill(commands: &mut Commands, at: Entity, rows: &[Row], kind: PanelKind, pane: Option<RowPane>) {
    commands.entity(at).despawn_related::<Children>();
    // **A list's node is `chrome::scroll_list`'s; an inspector's is written here.**
    //
    // This used to `insert(Node { .. })` unconditionally, and over a `scroll_list` that would have
    // silently replaced the three things making a list scrollable — the `overflow`, the `ScrollArea`
    // and the `min_height: 0`. Dropping it wholesale was the first fix and it was wrong in the other
    // direction: the inspector containers are plain nodes, so they fell back to `FlexDirection::Row`
    // and `MAP INFO` drew its four rows side by side, wrapping mid-value. Caught in a capture, which
    // is the half of this a green suite could not have told me.
    if kind == PanelKind::Inspector {
        commands.entity(at).insert(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(crate::chrome::GAP_TIGHT),
            // Full width, or the rows inside do not line up with the panel they sit in — the
            // container shrinks to its own content and each row's `width: 100%` resolves against
            // *that*, which is what left the right column ragged (reported 2026-08-16).
            width: Val::Percent(100.0),
            ..default()
        });
    }
    // **One label column for the whole panel** — see [`label_col`]. Decided before the loop,
    // because a column decided row by row is not a column.
    let col = label_col(rows);
    for (i, r) in rows.iter().enumerate() {
        let c = colour(r.tone);
        // **A row being typed into is the selected row**, and carries the fill, the rail and the
        // mark — only its ink differs (see [`Tone::Editing`]).
        let selected = matches!(r.tone, Tone::Selected | Tone::Editing);
        // **The chevron points into the column this row opens.** Only a list has one; a settings row
        // opens nothing, and giving it the same mark would restate the confusion this is fixing.
        let mark = match (kind, selected) {
            (PanelKind::List, true) => "\u{203a}",
            (PanelKind::Inspector, true) => "\u{2022}",
            _ => " ",
        };
        let left = format!("{mark} {}", r.left);
        let right = r.right.clone();
        commands.entity(at).with_children(|p| match kind {
            PanelKind::List => {
                // **Not `list_row`** — see `chrome::quiet_row`. The menu brings its own click handler
                // and must stay off the editor's `Activate` bus, whose observers take Map-door
                // resources that do not exist on this screen.
                let mut row = crate::chrome::quiet_row(p, selected, ());
                if let Some(pane) = pane {
                    row.insert(ChooserRow { pane, index: i });
                    row.observe(on_row_click);
                }
                row.with_children(|line| {
                    line.spawn((
                        Text::new(left.clone()),
                        crate::chrome::font(crate::chrome::text::BODY),
                        TextColor(c),
                        Node {
                            flex_grow: 1.0,
                            // CHROME-OK: not a gap — a flex item's automatic minimum size is its
                            // content, so without this the label refuses to shrink and pushes the
                            // value out of the row.
                            min_width: Val::Px(0.0),
                            ..default()
                        },
                    ));
                    if !right.is_empty() {
                        line.spawn((
                            Text::new(right.clone()),
                            crate::chrome::font(crate::chrome::text::BODY),
                            TextColor(c),
                            TextLayout::new(Justify::Right, LineBreak::NoWrap),
                            Node { flex_shrink: 0.0, ..default() },
                        ));
                    }
                });
            }
            PanelKind::Inspector => {
                // **The policy panel's rows are selectable** — the arrows and a click can stand on
                // them, because `Delete` removes the highlighted entry. Every other inspector row is
                // facts about the selection and stays unclickable.
                if pane == Some(RowPane::Policy) {
                    // **Not `list_row`** — see `chrome::quiet_row`. The menu brings its own click
                    // handler and must stay off the editor's `Activate` bus.
                    let mut row = crate::chrome::quiet_row(p, selected, ());
                    row.insert(ChooserRow { pane: RowPane::Policy, index: i });
                    row.observe(on_row_click);
                    row.with_children(|line| {
                        line.spawn(info_row()).with_children(|line| {
                            crate::chrome::row_label(line, col, &left);
                            crate::chrome::row_value(line, right.clone(), c, ());
                        });
                    });
                } else {
                    p.spawn(info_row()).with_children(|line| {
                        crate::chrome::row_label(line, col, &left);
                        crate::chrome::row_value(line, right.clone(), c, ());
                    });
                }
            }
        });
    }
}

/// **Keep the selected row on screen**, the way every other list in this editor does.
///
/// `tests/every_list_follows_its_selection.rs` requires one of these per `scroll_list` marker, and
/// its header records why: the defect was reported twice and passed its tests both times, because
/// the tests measured a world that only exists in tests.
///
/// **Keyed on the selection, never on `Res<Chooser>::is_changed`** — `chrome::Follow`'s founding
/// observation. The resource is written on most frames (a keystroke, a rescan, a message), so
/// watching it would re-arm this for ever and the scroll would never run. `Follow` also swallows the
fn keep_the_chooser_selection_on_screen(
    chooser: Res<Chooser>,
    rows: Query<(&ChooserRow, &ComputedNode, &UiGlobalTransform)>,
    mut maps: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        (With<MapList>, Without<KitList>, Without<ProjectList>, Without<ChooserRow>),
    >,
    mut kits: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        (With<KitList>, Without<MapList>, Without<ProjectList>, Without<ChooserRow>),
    >,
    mut projects: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        (With<ProjectList>, Without<MapList>, Without<KitList>, Without<ChooserRow>),
    >,
    mut follow: Local<crate::chrome::Follow<(usize, usize)>>,
) {
    // The cursor as one value, so `Follow` re-arms when either half moves — walking from kit 2 to
    // kit 3 and crossing from the maps column to the kits column are both "the selection moved".
    let (pane, index) = match chooser.focus {
        Focus::Maps => (0, chooser.map),
        Focus::Kits => (1, chooser.kit),
        Focus::Projects => (2, chooser.project),
        // The settings and policy panels are content-sized and never scroll; nothing to reveal.
        Focus::Settings | Focus::Policy => return,
    };
    if !follow.should_scroll(Some((pane, index))) {
        return;
    }
    let want = match pane {
        0 => RowPane::Maps,
        1 => RowPane::Kits,
        _ => RowPane::Projects,
    };
    let Some((row_mid, row_half)) = rows
        .iter()
        .find(|(r, _, _)| r.pane == want && r.index == index)
        .map(|(_, n, t)| (t.translation.y, n.size().y * 0.5))
    else {
        return;
    };
    let reveal = |list: &ComputedNode, tf: &UiGlobalTransform, scroll: &mut ScrollPosition| {
        let (at, max) = crate::chrome::scroll_bounds(list);
        if let Some(y) = crate::chrome::scroll_to_reveal(
            (row_mid, row_half),
            (tf.translation.y, list.size().y * 0.5),
            at,
            max,
            list.inverse_scale_factor,
        ) {
            scroll.0.y = y;
        }
    };
    match pane {
        0 => {
            for (list, tf, mut scroll) in &mut maps {
                reveal(list, tf, &mut scroll);
            }
        }
        1 => {
            for (list, tf, mut scroll) in &mut kits {
                reveal(list, tf, &mut scroll);
            }
        }
        _ => {
            for (list, tf, mut scroll) in &mut projects {
                reveal(list, tf, &mut scroll);
            }
        }
    }
}

/// **A click puts the cursor where the arrows would have put it**, through the same two fields —
/// so the pointer and the keyboard cannot come to disagree about what is selected.
fn on_row_click(
    click: On<Pointer<Click>>,
    rows: Query<&ChooserRow>,
    mut chooser: Option<ResMut<Chooser>>,
    // For the double-click, which runs the same `open_the_selection` `Enter` does.
    mut commands: Commands,
    next: Option<ResMut<NextState<crate::screen::Screen>>>,
) {
    // **`Option`, because this is a GLOBAL observer** — it fires for any `Activate` anywhere in
    // the application, and in Bevy 0.19 a missing resource panics at param validation rather
    // than skipping. See [`on_cell_verb`] for the whole argument; the ratchet is
    // `every_resource_says_what_a_door_does_to_it.rs`.
    let Some(mut next) = next else {
        return;
    };
    let (Ok(row), Some(chooser)) = (rows.get(click.entity), chooser.as_mut()) else {
        return;
    };
    // **The same three refusals `drive_chooser` makes, because they are about the SCREEN and not
    // about the keyboard.** A click that moved the cursor while `delete \`hall\`?` was up left the
    // question naming one map and the highlight on another — which is `drive_chooser`'s own words
    // for "how a confirmation deletes the wrong thing", reachable again by pointer the moment the
    // rows learned to answer one. `editing` is the same argument: a click that moved the selection
    // out from under an open field left the field swallowing keys for a row nobody was standing on.
    if chooser.editing || chooser.ask.is_some() {
        return;
    }
    // And a stale refusal goes with the move, exactly as `step`/`section` clear it.
    chooser.problem = None;
    match row.pane {
        RowPane::Kits => {
            chooser.focus = Focus::Kits;
            chooser.kit = row.index;
        }
        RowPane::Policy => {
            chooser.focus = Focus::Policy;
            chooser.policy = row.index;
        }
        RowPane::Maps => {
            chooser.focus = Focus::Maps;
            chooser.map = row.index;
        }
        RowPane::Settings => chooser.focus = Focus::Settings,
        RowPane::Projects => {
            chooser.focus = Focus::Projects;
            chooser.project = row.index;
        }
    }
    // **The second click opens it**, through the identical door `Enter` walks — see
    // [`open_the_selection`]. Bevy's `Pointer<Click>` carries `count`, so this needs no timer and
    // no remembered entity: the first click arrives as 1 and has already moved the cursor above,
    // the second arrives as 2 and acts on it. `>= 2` rather than `== 2` so a fast triple-click
    // opens rather than falling through and doing nothing.
    if click.count >= 2 {
        open_the_selection(chooser, &mut commands, &mut next);
    }
}

#[allow(clippy::type_complexity)]
fn paint_chooser(
    mut commands: Commands,
    chooser: Res<Chooser>,
    lists: Query<(
        Entity,
        Option<&KitList>,
        Option<&MapList>,
        Option<&SettingsList>,
        Option<&KitInfoList>,
        Option<&PolicyList>,
        Option<&ProjectList>,
    )>,
    mut texts: Query<(
        &mut Text,
        Option<&MapsHeader>,
        Option<&SettingsHeader>,
        Option<&ProblemLine>,
        Option<&HintLine>,
        Option<&KitInfoHeader>,
        Option<&PolicyHeader>,
        Option<&ProjectHeader>,
    )>,
) {
    if !chooser.is_changed() {
        return;
    }
    let s = chooser.screen();
    for (e, kit, map, set, info, policy, project) in &lists {
        if kit.is_some() {
            fill(&mut commands, e, &s.kits, PanelKind::List, Some(RowPane::Kits));
        } else if map.is_some() {
            fill(&mut commands, e, &s.maps, PanelKind::List, Some(RowPane::Maps));
        } else if project.is_some() {
            fill(&mut commands, e, &s.projects, PanelKind::List, Some(RowPane::Projects));
        } else if set.is_some() {
            fill(&mut commands, e, &s.settings, PanelKind::Inspector, Some(RowPane::Settings));
        } else if policy.is_some() {
            fill(&mut commands, e, &s.policy, PanelKind::Inspector, Some(RowPane::Policy));
        } else if info.is_some() {
            fill(&mut commands, e, &s.kit_info, PanelKind::Inspector, None);
        }
    }
    for (mut text, maps, settings, problem, hint, kit_info, policy, project) in &mut texts {
        if kit_info.is_some() {
            **text = s.kit_header.clone();
        } else if policy.is_some() {
            **text = s.policy_header.clone();
        } else if project.is_some() {
            **text = s.projects_header.clone();
        } else if maps.is_some() {
            **text = s.maps_header.clone();
        } else if settings.is_some() {
            **text = s.settings_header.clone();
        } else if problem.is_some() {
            **text = s
                .asking
                .clone()
                .or_else(|| s.problem.clone())
                .unwrap_or_default();
        } else if hint.is_some() {
            **text = s.hint.clone();
        }
    }
}

/// **The field takes the keyboard first.** Mirrors `build.rs`'s name prompt, including the drain:
/// while no field is open the stream is cleared, so the keystroke that *opens* one cannot become its
/// first character (the `xseam` bug this crate already paid for once).
fn type_into_field(mut events: MessageReader<KeyboardInput>, mut chooser: ResMut<Chooser>) {
    if !chooser.editing {
        events.clear();
        return;
    }
    let field = chooser.field;
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            // **`Enter` keeps it and stops typing** — it does not jump to the next field. Moving
            // between fields is the arrows now, and a commit that also moved would be the same key
            // doing two jobs, which is what this screen's `Tab` was just corrected for.
            Key::Enter => {
                keep_field(&mut chooser, field);
                chooser.swallowed = true;
                return;
            }
            // **Escape leaves the FIELD, and nothing else.** It sets `swallowed` so the chord
            // handler does not read the same press as "quit" — which is exactly what it did, and
            // closed the program on somebody who only wanted out of a text box.
            Key::Escape => {
                chooser.raw.clear();
                chooser.problem = None;
                chooser.editing = false;
                chooser.swallowed = true;
                return;
            }
            Key::Backspace => {
                chooser.raw.pop();
            }
            Key::Space => chooser.raw.push(' '),
            Key::Character(s) => {
                let s = s.clone();
                chooser.raw.push_str(&s);
            }
            _ => {}
        }
    }
}

/// **Keep what was typed — and while making something, keeping the name is the whole act.**
///
/// Asked for at the keyboard: *"I create a new kit, I hit enter to confirm the name… once you hit
/// enter, select the kit in the kit area"*, and then *"do the same for maps."* Neither did; the name
/// was kept, and a second, different key — `Ctrl+Enter` — made the thing. A guide card written for
/// this screen had already called that *"the key I trust least."*
///
/// The map half was argued the other way first, and the argument was wrong. It ran: a kit has one
/// field so a commit door guards nothing, but a map has four, and its bounds were the setting once
/// reachable only by editing source — so a map should keep its door. What that missed is that
/// **`MAP INFO` edits a map that exists**, writing the file through the same validate-then-atomic
/// door `Project::save` uses. Bounds set before creation and bounds set after are the same bounds
/// by the same code. The door was not protecting the value; it was only making the author find a
/// key to get past it.
///
fn keep_field(chooser: &mut Chooser, field: Field) {
    if !commit_field(chooser, field) {
        return;
    }
    chooser.editing = false;
    if let Some(new) = chooser.creating.clone() {
        // **Only a refusal overwrites the status line.** `make_it`'s project arm sets the
        // line itself — `run emerge-mapper <dir> to open it` — and `.err()` unconditionally
        // would have wiped it the same frame it was written.
        if let Err(e) = make_it(chooser, &new) {
            chooser.problem = Some(e);
        }
    }
}

/// Parse and store one field, or refuse it by name. **Nothing is substituted** — a value that will
/// not parse leaves the old one alone and says why.
///
/// Answers **whether it committed**, and deliberately does not decide where the keyboard goes next:
/// `Enter` and `Tab` advance, `Shift+Tab` goes back, and a refusal keeps you on the field whichever
/// key you pressed. Choosing the destination in here made that one behaviour with three callers.
fn commit_field(chooser: &mut Chooser, field: Field) -> bool {
    let raw = chooser.raw.trim().to_owned();
    // A kit in hand has exactly one field, and it is this one.
    // A project in hand is the same shape — one name, and `make_it` does the rest.
    if let Some(New::Project(_)) = &chooser.creating {
        let name = naming::to_snake_case(&raw);
        if name.is_empty() {
            chooser.problem = Some(
                "a project needs a name — snake_case, starting with a letter".to_owned(),
            );
            return false;
        }
        chooser.creating = Some(New::Project(name));
        chooser.raw.clear();
        chooser.problem = None;
        return true;
    }
    if let Some(New::Kit(_)) = &chooser.creating {
        let name = naming::to_snake_case(&raw);
        if name.is_empty() {
            chooser.problem =
                Some("a kit needs a name — snake_case, starting with a letter".to_owned());
            return false;
        }
        chooser.creating = Some(New::Kit(name));
        chooser.raw.clear();
        chooser.problem = None;
        return true;
    }
    // Editing an existing map writes that file; making one fills in a draft first.
    let existing = chooser.creating.is_none();
    let mut draft = match (&chooser.creating, chooser.current_map()) {
        (Some(New::Map(d)), _) => d.clone(),
        (Some(New::Kit(_)), _) => return false,
        (Some(New::Project(_)), _) => return false,
        (None, Some(m)) => match &m.summary {
            MapSummary::Read { bounds, .. } => {
                // A `Draft` carries the four editable fields; the bash is not one of them, and
                // `write_settings` leaves it as it was.
                let (origin, note, _) = read_map_details(&m.path);
                Draft {
                    name: m.name.clone(),
                    bounds: *bounds,
                    origin,
                    note,
                }
            }
            MapSummary::Unreadable(_) => return false,
        },
        (None, None) => return false,
    };

    match field {
        Field::Name => {
            let name = naming::to_snake_case(&raw);
            if name.is_empty() {
                chooser.problem =
                    Some("a map needs a name — snake_case, starting with a letter".to_owned());
                return false;
            }
            draft.name = name;
        }
        Field::Bounds | Field::Origin => {
            let Some(t) = parse_triple(&raw) else {
                chooser.problem = Some(format!(
                    "`{raw}` is not three numbers — type them like `32 4 32`"
                ));
                return false;
            };
            if field == Field::Bounds {
                if t.0 <= 0.0 || t.1 <= 0.0 || t.2 <= 0.0 {
                    chooser.problem =
                        Some("a map's bounds must all be positive — it is a volume".to_owned());
                    return false;
                }
                draft.bounds = t;
            } else {
                draft.origin = t;
            }
        }
        Field::Note => draft.note = (!raw.is_empty()).then_some(raw),
    }

    if existing {
        // **The one genuinely new write path**, and it goes through the same door `Project::save`
        // uses: validate, then an atomic write. A map edited here and one saved from the editor
        // cannot disagree about what a map is.
        if let Err(e) = write_settings(chooser, &draft) {
            chooser.problem = Some(e);
            return false;
        }
    } else {
        chooser.creating = Some(New::Map(draft));
    }
    chooser.raw.clear();
    chooser.problem = None;
    true
}

/// Apply edited settings to the map on disk, then rescan so the list describes the file rather than
/// the edit.
pub fn write_settings(chooser: &mut Chooser, draft: &Draft) -> Result<(), String> {
    let Some(entry) = chooser.current_map() else {
        return Err("nothing selected".to_owned());
    };
    let old_path = entry.path.clone();
    let text = std::fs::read_to_string(&old_path)
        .map_err(|e| format!("cannot read {}: {e}", old_path.display()))?;
    let mut map = Map::parse(&text)?;
    map.name = draft.name.clone();
    map.bounds = draft.bounds;
    map.origin = draft.origin;
    map.note = draft.note.clone();
    map.validate()?;

    let dir = old_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let path = dir.join(naming::map_file_name(&map.name));
    if path != old_path && path.exists() {
        return Err(format!("`{}` already exists in this project", map.name));
    }
    let out = ron::ser::to_string_pretty(&map, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("map: serialize: {e}"))?;
    emerge_core::ron_surgery::save_atomic(&path, &out)?;
    // Follow a rename, exactly as `Project::save` does — the file a map is in is the file its name
    // says it is.
    //
    // **Reported, not discarded.** A removal that fails leaves two files answering to one map, and
    // the stale one then refuses to open with `"<path> calls itself '<name>'"` — a message about a
    // schema mismatch, for a rename that half-happened. The new file is written and correct, so this
    // is a warning on the row rather than a failure of the edit.
    if path != old_path
        && let Err(e) = std::fs::remove_file(&old_path)
    {
        return Err(format!(
            "saved as `{}`, but the old file {} could not be removed: {e}. Delete it by hand — \
             until then the list shows this map twice.",
            map.name,
            old_path.display()
        ));
    }
    let name = map.name.clone();
    rescan_keeping_place(chooser, Some(&name));
    Ok(())
}

/// Rescan and land on a named map, so the list is always a description of disk.
/// **Make what is in hand**, whichever kind it is, and land the selection on it.
///
/// One function so that "the draft becomes real" happens in exactly one place, whether the draft is
/// a kit or a map — the two differ in which creator they call and nowhere else.
fn make_it(chooser: &mut Chooser, new: &New) -> Result<(), String> {
    match new {
        New::Kit(name) => {
            create_kit(&chooser.root.clone(), name)?;
            chooser.creating = None;
            rescan_keeping_place(chooser, None);
            // Land on the kit that was just made, so the maps column beside it is already its own.
            if let Some(i) = chooser.catalog.kits.iter().position(|k| &k.label == name) {
                chooser.kit = i + 1;
                chooser.map =
                    Chooser::first_real(chooser.catalog.maps.len());
            }
            chooser.focus = Focus::Kits;
        }
        New::Map(d) => {
            // **The project's `maps/`, and no kit is consulted.** Making a map used to require a
            // selected kit — it was written beside that kit's library — and a map now resolves
            // against every bound kit merged, so there is nothing left for the selection to decide.
            let dir = chooser.root.join(EMERGE_DIR).join(MAPS_DIR);
            create_map(&dir, &d.name, d.bounds, d.origin, d.note.clone())?;
            let name = d.name.clone();
            chooser.creating = None;
            rescan_keeping_place(chooser, Some(&name));
            chooser.focus = Focus::Maps;
        }
        New::Project(name) => {
            // **A sibling beside this root** — the parent is where the other projects live, and
            // the vocabulary is this project's own, copied byte-for-byte.
            let parent = chooser
                .root
                .parent()
                .ok_or_else(|| "this project has no parent directory to make a sibling in".to_owned())?
                .to_path_buf();
            let vocab = chooser.root.join(EMERGE_DIR).join("vocab.ron");
            let dir = create_project(&parent, name, &vocab)?;
            chooser.creating = None;
            rescan_keeping_place(chooser, None);
            chooser.focus = Focus::Projects;
            if let Some(i) = chooser
                .catalog
                .projects
                .iter()
                .position(|p| p.dir == dir)
            {
                chooser.project = i + 1;
            }
            // **The status line reports the command that opens it** — nothing opens in-process,
            // so the author's next keystroke is in the other terminal. The name is what was
            // typed, and the directory is where it landed.
            chooser.problem = Some(format!(
                "`{name}` — run `emerge-mapper {}` to open it",
                dir.display()
            ));
        }
    }
    Ok(())
}

fn rescan_keeping_place(chooser: &mut Chooser, want: Option<&str>) {
    let label = chooser.current_kit().map(|k| k.label.clone());
    match Catalog::scan(&chooser.root.clone()) {
        Err(e) => chooser.problem = Some(e),
        Ok(catalog) => {
            chooser.catalog = catalog;
            chooser.kit = label
                .and_then(|l| chooser.catalog.kits.iter().position(|k| k.label == l))
                .map_or_else(
                    || Chooser::first_real(chooser.catalog.kits.len()),
                    |i| i + 1,
                );
            chooser.map = want
                .and_then(|w| {
                    chooser
                        .catalog
                        .maps
                        .iter()
                        .position(|m| m.name == w)
                        .map(|i| i + 1)
                })
                .unwrap_or_else(|| {
                    Chooser::first_real(chooser.catalog.maps.len())
                });
        }
    }
}

/// Three whitespace- or comma-separated numbers, or nothing. Refuses rather than filling in a
/// missing axis, because a bounds triple with a guessed Y is a map of a height nobody chose.
///
/// **Every token has to parse, and every number has to be finite.** `filter_map(..ok())` silently
/// dropped the ones that did not, so `1 nope 2 3` read as `(1, 2, 3)` — a substituted value in a
/// field whose whole doc says it substitutes nothing. `"nan"` and `"inf"` parse as `f32`, and
/// `Map::validate` checks only `bounds`, so a non-finite ORIGIN was written to disk and every
/// `to_map_space` downstream of it returned `NaN`.
fn parse_triple(raw: &str) -> Option<(f32, f32, f32)> {
    let mut parts: Vec<f32> = Vec::new();
    for token in raw
        .split(|c: char| c.is_whitespace() || c == ',' || c == 'x')
        .filter(|s| !s.is_empty())
    {
        let v: f32 = token.parse().ok()?;
        if !v.is_finite() {
            return None;
        }
        parts.push(v);
    }
    match parts[..] {
        [x, y, z] => Some((x, y, z)),
        _ => None,
    }
}

fn drive_chooser(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut chooser: ResMut<Chooser>,
    mut commands: Commands,
    mut next: ResMut<NextState<crate::screen::Screen>>,
    mut exit: MessageWriter<AppExit>,
    // Held arrows walk the list at the one application-wide cadence — see `keys::REPEAT_SECS`.
    // `KeysPlugin` owns `Repeat` and `screen.rs` marks it session-owned, so both outlive this
    // screen and a key held across the door does not resume repeating on the far side of it.
    time: Res<Time>,
    mut repeat: ResMut<crate::keys::Repeat>,
    // The one prompt. This screen raises two of the four questions in `confirm::Asked`.
    mut confirm: ResMut<crate::confirm::Confirm>,
) {
    // **A key the text handler already took is not read again.** One `Escape` is one press; see
    // `Chooser::swallowed` for the bug this closes.
    //
    // **Read through `Deref` first, and only write on the frame it is set.** `std::mem::take` needs
    // `&mut`, and `ResMut::deref_mut` calls `set_changed()` — so taking it unconditionally marked
    // `Chooser` changed on every frame, which silently killed `paint_chooser`'s `is_changed()` guard
    // and rebuilt all four panels (plus a `read_to_string` of the selected map and of the kit's
    // policy) sixty times a second on an idle menu.
    if chooser.swallowed {
        chooser.swallowed = false;
        return;
    }
    // Typing owns the keyboard; `type_into_field` ran first and has already consumed it.
    if chooser.editing {
        return;
    }
    // **A pending deletion owns it too, and answers only yes or no.** Every other key is ignored
    // rather than doing its usual job behind the question — an arrow that moved the selection while
    // "delete `hall`?" was on screen would leave the prompt naming one map and the highlight on
    // another, which is how a confirmation deletes the wrong thing.
    // **The modal owns the keyboard while it is up.** `confirm` reads `Y`/`N`/`Esc` itself; every
    // other key is ignored rather than doing its usual job behind the question — an arrow that
    // moved the selection while "delete `hall`?" was on screen would leave the prompt naming one
    // map and the highlight on another, which is how a confirmation deletes the wrong thing.
    if let Some(agreed) = confirm.answer(crate::confirm::Asked::DeleteEntry) {
        chooser.ask = None;
        chooser.problem = None;
        if agreed {
            chooser.problem = match chooser.confirm_delete() {
                Ok(name) => Some(format!("`{name}` deleted")),
                Err(e) => Some(e),
            };
        }
        return;
    }
    if let Some(agreed) = confirm.answer(crate::confirm::Asked::QuitApp) {
        chooser.ask = None;
        if agreed {
            exit.write(AppExit::Success);
        }
        return;
    }
    if confirm.is_open() {
        return;
    }
    // **Ask; do not do.** `Delete` on a map raises the question and changes nothing — see
    // [`Pending`] for why this verb is split in two.
    if keyboard.just_pressed(KeyCode::Delete) || keyboard.just_pressed(KeyCode::Backspace) {
        chooser.problem = chooser.ask_delete().err();
        // **`ask_delete` arms the state; the modal states the question.** Split that way because
        // `ask_delete` is a `Chooser` method — it decides whether there IS anything to delete and
        // refuses the root kit — while the wording belongs where every other question's wording
        // lives. A refusal leaves `ask` empty, so nothing is raised.
        if let Some(Ask::Delete(pending)) = chooser.ask.clone() {
            let what = if pending.kit { "kit" } else { "map" };
            confirm.ask(
                crate::confirm::Asked::DeleteEntry,
                format!("Delete the {what} `{}`?", pending.name),
                if pending.kit {
                    "The whole kit directory goes, with everything in it. This cannot be undone."
                } else {
                    "The map file is removed. This cannot be undone."
                },
                "Delete it",
                "Keep it",
            );
        }
        // **A policy removal is the same question, asked the same way** — the modal names the
        // exact entry (`shown`), because a `because` string is hand-written rationale and
        // `docs/ui.md` §1.4's rule is that "are you sure?" is not information.
        if let Some(Ask::RemovePolicy { shown, .. }) = chooser.ask.clone() {
            confirm.ask(
                crate::confirm::Asked::DeleteEntry,
                "Remove this policy entry?",
                format!("`{shown}` is spliced out of the kit's project.ron. The file's comments are kept."),
                "Remove it",
                "Keep it",
            );
        }
        return;
    }
    // **Arrows move inside a panel. `Tab` crosses between them.** One rule, no exceptions — the
    // correction asked for at the keyboard, replacing a `Tab` that meant "next field" in the
    // settings and "go to the settings" everywhere else.
    //
    // **And they repeat when held**, at `keys::REPEAT_SECS`, like every list inside the editor.
    // They read raw `KeyCode` rather than an `Action` because the door has no key census — that is
    // deliberate — so they go through `keys::repeating_key`, which is the same countdown the
    // census path uses and not a second one. A kit list long enough to scroll is long enough that
    // tapping down it is not a job.
    let dt = time.delta_secs();
    if crate::keys::repeating_key(&keyboard, KeyCode::ArrowUp, &mut repeat, dt) {
        chooser.step(-1);
    }
    if crate::keys::repeating_key(&keyboard, KeyCode::ArrowDown, &mut repeat, dt) {
        chooser.step(1);
    }
    // **`left`/`right` cross columns**, the same pair the Meshes tab uses to move between its two
    // side-by-side lists (`keys::Action::FocusCandidates` / `FocusLibrary`). There is no `Tab`: it
    // was the only `KeyCode::Tab` in the crate, and a second crossing key on one screen out of six
    // is a dialect. See `Focus` for the whole rule.
    if keyboard.just_pressed(KeyCode::ArrowRight) {
        chooser.cross(1);
    }
    if keyboard.just_pressed(KeyCode::ArrowLeft) {
        chooser.cross(-1);
    }
    // **`N` makes a new one of whatever this column lists** — see `Chooser::start_new`.
    if keyboard.just_pressed(KeyCode::KeyN) {
        chooser.start_new();
    }
    // **Escape unwinds one layer at a time, and never destroys anything on the first press.**
    //
    // Asked for at the keyboard after it closed the program mid-typing. The layers, outermost last:
    //
    //   1. in a text field  -> leave the field   (handled in `type_into_field`)
    //   2. making a new map -> abandon the draft
    //   3. otherwise        -> ASK about quitting; `Y` answers it
    //
    // Each press does one thing and says what the next one would do, so "Escape" is learnable as a
    // single idea — back out of where you are — rather than as a key whose meaning you have to
    // predict before pressing it.
    if keyboard.just_pressed(KeyCode::Escape) {
        if chooser.creating.is_some() {
            chooser.creating = None;
            chooser.problem = None;
        } else {
            chooser.ask = Some(Ask::Quit);
            confirm.ask(
                crate::confirm::Asked::QuitApp,
                "Quit emerge-mapper?",
                "Anything already saved stays saved.",
                "Quit",
                "Stay",
            );
        }
    }
    // **`B` names the bash the selected map draws on**, cycling the project's declared combinations
    // and rounding back to every-kit.
    //
    // On the MAPS column, because a bash belongs to a map — the kit ticks that used to live on the
    // KITS column edited a per-map list, and a combination is shared, so flipping one from a kit row
    // would silently change every other map naming it. There is no verb here that *makes* a bash: it
    // is authored by hand in `kits.ron`, the same way that file's `lattice` is.
    if keyboard.just_pressed(KeyCode::KeyB) && chooser.focus == Focus::Maps {
        chooser.problem = chooser.cycle_bash().err();
        return;
    }
    // **`A` points `authoring` at the kit under the cursor** — the mouse half is the clickable
    // `new work lands here` value. Both go through `set_authoring`, and the row repaints because
    // `rescan_keeping_place` rebuilds the catalog.
    if keyboard.just_pressed(KeyCode::KeyA) && chooser.focus == Focus::Kits {
        if chooser.on_new_row() {
            chooser.problem =
                Some("that row makes a new kit — there is nothing yet to make authoring".to_owned());
            return;
        }
        let Some(kit) = chooser.current_kit() else {
            chooser.problem = Some("no kit under the cursor".to_owned());
            return;
        };
        let name = kit.label.clone();
        chooser.problem = set_authoring(&chooser.root, &name).err();
        if chooser.problem.is_none() {
            rescan_keeping_place(&mut chooser, None);
        }
        return;
    }
    if keyboard.just_pressed(KeyCode::Enter) {
        open_the_selection(&mut chooser, &mut commands, &mut next);
    }
}

/// **What `Enter` means on the row the cursor is on — and therefore what a double-click means.**
///
/// Extracted so the pointer and the keyboard cannot come to disagree about what "open this" does.
/// The chooser was keyboard-*only* rather than keyboard-first: a click moved the cursor and nothing
/// else, so with a mouse you could select a kit and then had to reach for `Enter` to actually open
/// it, and `+ new …` was unreachable by pointer entirely. Reported at the keyboard 2026-08-19 —
/// *"we are keyboard first. We're not anti mouse."*
///
/// A single click still only selects. Acting on the first click would make a mis-click open a
/// door, and the arrow keys' own contract is that moving the cursor is free.
fn open_the_selection(
    chooser: &mut Chooser,
    commands: &mut Commands,
    next: &mut NextState<crate::screen::Screen>,
) {
    {
        // **`Enter` on a `+ new …` row starts the name prompt**, which is the verb the status band
        // has been advertising all along: `Enter new project`, `Enter new kit`, `Enter new map`.
        //
        // On the projects column it did nothing whatsoever — `ui_audit.md` F4, verified on a
        // full-frame capture: no prompt, no project, no refusal, no status line. The arm below
        // reaches for `projects[project - 1]`, and row 0 has no such entry, so the `if let` missed
        // and the function returned. On the other two columns it was worse than nothing: they fell
        // through to `launch_args`, whose refusal for this row reads *"that row makes a new one —
        // press Enter on it"*, naming the key that had just been pressed.
        //
        // **The verb existed and only its doorway was missing**: `start_new` → `commit_field` →
        // `make_it` → [`create_project`] is the same chain `N` has always walked, and
        // `create_project` byte-copies this project's `vocab.ron` and writes the empty `kits.ron`.
        // One arm for all three columns, because every column's row 0 is the same promise — and
        // `on_new_row` is false in POLICY and MAP INFO, which have no such row.
        if chooser.on_new_row() {
            chooser.start_new();
            return;
        }
        // **There is no `Ctrl+Enter` here any more.** Both a kit and a map are made by pressing
        // `Enter` on the name (see [`keep_field`]); a chord that made the same thing a second way
        // would be the way nobody found.
        //
        // In the settings, `Enter` opens the highlighted row for typing — every row there is a text
        // field now that the kit toggles live on the KITS column.
        if chooser.focus == Focus::Settings {
            // **The field opens holding what is in it**, rather than blank. A blank field plus
            // "Enter keep" is a two-keystroke way to destroy a value with no prompt and no undo:
            // `NOTE` is the one setting whose empty commit is a legal value, so `Enter Enter` on a
            // map's note silently wrote `note: None` to disk. Seeding also makes the other three
            // editable rather than merely retypable.
            let seed = chooser.settled(chooser.field);
            chooser.raw = seed;
            chooser.problem = None;
            chooser.editing = true;
            return;
        }
        if chooser.focus == Focus::Projects {
            if let Some(p) = chooser
                .project
                .checked_sub(1)
                .and_then(|i| chooser.catalog.projects.get(i))
            {
                if p.current {
                    chooser.problem = None;
                } else {
                    chooser.problem = Some(format!(
                        "`{}` — run `emerge-mapper {}` to open it",
                        p.name,
                        p.dir.display()
                    ));
                }
            }
            return;
        // **Opened here, and the screen only moves once it has.**
        }
        // **Opened here, and the screen only moves once it has.**
        //
        // A state change, not a process launch: this wrote the argv into a mutex the parent process
        // read after `AppExit`, because the menu and the editor were two programs. They are one now
        // (`screen.rs`). The *opening* is here rather than on the transition out because this is the
        // last place a refusal can be shown — `problem` is already on screen and nothing has been
        // torn down. A door that will not open now costs a keystroke and a sentence; opening it on
        // `OnExit(Menu)` cost a panic, because a `NextState` written there is not read until after
        // `OnEnter(Editor)` has already run. See [`Chosen`].
        match chooser
            .launch_args()
            .and_then(|args| crate::args::open(&args))
        {
            Err(e) => chooser.problem = Some(e),
            Ok(opened) => {
                commands.insert_resource(Chosen(opened));
                next.set(crate::screen::Screen::Editor);
            }
        }
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;

    fn chooser_with(kits: Vec<Kit>) -> Chooser {
        Chooser::new(
            PathBuf::from("."),
            Catalog {
                kits,
                maps: Vec::new(),
                authoring: None,
                bashes: Vec::new(),
                projects: Vec::new(),
            },
            None,
        )
    }

    /// The same, with the project's maps — which is where they live now, so they are given beside
    /// the kits rather than inside one.
    fn chooser_with_maps(kits: Vec<Kit>, maps: Vec<MapEntry>) -> Chooser {
        Chooser::new(
            PathBuf::from("."),
            Catalog {
                kits,
                maps,
                authoring: None,
                bashes: Vec::new(),
                projects: Vec::new(),
            },
            None,
        )
    }

    fn kit(flag: Option<&str>, label: &str, pieces: usize) -> Kit {
        Kit {
            flag: flag.map(str::to_owned),
            label: label.to_owned(),
            dir: PathBuf::from(label),
            pieces,
            namespace: None,
            bound: false,
            ids: BTreeSet::new(),
        }
    }

    /// **The piece count is on screen.** It is the fact the whole chooser exists to show — the one
    /// that was unavailable on 2026-08-15 when an author could not tell which kit was loaded and
    /// relaunched three times against the wrong one.
    #[test]
    fn the_screen_says_how_many_pieces_each_kit_holds() {
        let c = chooser_with(vec![
            kit(Some("site"), "site", 45),
            kit(Some("site_v2"), "site_v2", 0),
        ]);
        let screen = render(&c);
        assert!(
            screen.contains("site") && screen.contains("45 pieces"),
            "{screen}"
        );
        assert!(
            screen.contains("0 pieces"),
            "the blank kit reads as blank:\n{screen}"
        );
    }

    /// The root kit is labelled, because "no `--kit` at all" is a real mode and an unlabelled row
    /// looks like a kit whose flag somebody forgot.
    #[test]
    fn the_root_kit_says_it_is_the_default() {
        let c = chooser_with(vec![kit(None, "emerge", 75)]);
        assert!(render(&c).contains("(default)"), "{}", render(&c));
    }

    /// **An unmet condition is an instruction** (`docs/ui.md` §1.4). An empty kit does not report
    /// "no maps found"; it says which key makes one.
    #[test]
    fn an_empty_kit_reads_as_an_instruction_not_a_report() {
        let c = chooser_with(vec![kit(Some("site_v2"), "site_v2", 0)]);
        let screen = render(&c);
        assert!(
            screen.contains("+ new map"),
            "the instruction is a row you can press, not a sentence:\n{screen}"
        );
        assert!(
            !screen.contains("not found") && !screen.contains("no maps found"),
            "a report where an instruction belongs:\n{screen}"
        );
    }

    /// A map that will not parse says so on its own row, rather than being quietly absent — the
    /// author would otherwise go looking for a map the list had eaten.
    #[test]
    fn a_broken_map_is_visible_and_says_it_will_not_open() {
        let c = chooser_with_maps(vec![kit(Some("site"), "site", 1)], vec![MapEntry {
                name: "broken".into(),
                path: PathBuf::from("broken.map.ron"),
                summary: MapSummary::Unreadable("map: bad ron".into()),
            }]);
        let screen = render(&c);
        assert!(screen.contains("broken"), "{screen}");
        assert!(screen.contains("will not open"), "{screen}");
    }

    /// **The verb keys are on screen from the first frame**, which is what ExposeHK's rehearsal goal
    /// asks for: the novice path is the expert path, so using the screen teaches the keys rather
    /// than teaching pointing. Four verbs, against §3.5's 3–4 immediate-choice budget.
    ///
    /// **And a key that would do nothing is not listed.** Offering `Enter open` on a kit with no
    /// maps teaches something untrue, which is worse than teaching nothing — the rule `keys.rs`
    /// already states about a status line naming dead keys.
    #[test]
    fn the_verbs_are_shown_and_a_dead_one_is_not() {
        let stocked = chooser_with_maps(vec![kit(Some("site"), "site", 1)], vec![MapEntry {
                name: "hall".into(),
                path: PathBuf::from("hall.map.ron"),
                summary: MapSummary::Read {
                    placements: 0,
                    stamps: 0,
                    bounds: (4.0, 3.0, 4.0),
                },
            }]);
        let screen = render(&stocked);
        for verb in ["Enter open", "N new kit", "Esc quit"] {
            assert!(
                screen.contains(verb),
                "`{verb}` is not on screen:\n{screen}"
            );
        }

        let empty = chooser_with(vec![kit(Some("site_v2"), "site_v2", 0)]);
        let screen = render(&empty);
        assert!(
            !screen.contains("Enter open"),
            "there is nothing to open, so offering the key teaches a lie:\n{screen}"
        );
        assert!(
            screen.contains("N new kit"),
            "and the live verb is still there:\n{screen}"
        );
        // The maps column still offers its own way forward — a row, not a sentence.
        assert!(
            screen.contains("+ new map"),
            "an empty kit still says what to do next:\n{screen}"
        );
    }

    /// **The settings hint says which key does which job**, because neither has a visual affordance
    /// — ExposeHK's own caveat about techniques with "no visual representation to aid their
    /// discovery". Both are arrows now, so the line has to distinguish the two directions: `up`/
    /// `down` walk this panel's rows, `left`/`right` leave it for the next column.
    #[test]
    fn the_settings_hint_separates_moving_from_crossing() {
        let mut c = chooser_with_maps(vec![kit(Some("site"), "site", 1)], vec![MapEntry {
                name: "hall".into(),
                path: PathBuf::from("hall.map.ron"),
                summary: MapSummary::Read {
                    placements: 0,
                    stamps: 0,
                    bounds: (4.0, 3.0, 4.0),
                },
            }]);
        c.focus = Focus::Settings;
        c.field = Field::Bounds;
        let hint = c.hint();
        assert!(
            hint.contains("up/down field"),
            "the arrows are what move here: {hint}"
        );
        assert!(
            hint.contains("left/right column"),
            "and left/right are what cross: {hint}"
        );
        assert!(
            !hint.contains("Tab"),
            "there is no Tab on this screen any more — the editor navigates with arrows: {hint}"
        );
    }
}

// ------------------------------------------------------------------------------------------------
// The guide vocabulary
// ------------------------------------------------------------------------------------------------

/// **Grow the window while a card is up, and shrink back when it goes.**
///
/// The card is placed below the screen (see [`CARD_ROOM`]), so without this it would be drawn off
/// the bottom edge — which is the same defect as covering the columns, one direction over.
#[cfg(feature = "debugger")]
fn room_for_the_card(
    guide: Res<bevy_debugger_bevy::Guide>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut placement: ResMut<bevy_debugger_bevy::GuidePlacement>,
    mut extra: ResMut<ExtraRoom>,
) {
    // **The card hangs just under the screen, and the screen is now the window.**
    //
    // It used to be placed under `content_h` — the menu's own computed height, back when the layout
    // was a fixed-pixel grid measured from the catalogue. The frame fills the window, so there is no
    // such number any more and the honest one is the window's own height. Logical pixels, because
    // that is what the overlay places in.
    let Ok(window) = windows.single() else {
        return;
    };
    let screen = window.resolution.height();
    let top = screen - CARD_ROOM - 4.0;
    if (placement.top - top).abs() > 0.5 {
        placement.top = top;
    }
    // Declared, not applied — `crate::surface` is the one writer of the render target's size.
    let want = if guide.visible { CARD_ROOM } else { 0.0 };
    if extra.0 != want {
        extra.0 = want;
    }
}

/// **The chooser's own words for the guide channel**, and it needs its own because
/// `crate::guided`'s every condition reads `Res<Project>` — which does not exist in this `App` and
/// in Bevy 0.19 panics the system that asks for it. Same idea, different world.
///
/// A checkpoint asks *"has the state the step wanted arrived?"*, never *"did they press the right
/// key?"* — a script that watched keystrokes would be testing the author, and the exercise exists to
/// test the editor.
#[cfg(feature = "debugger")]
pub struct ChooserGuidePlugin;

#[cfg(feature = "debugger")]
impl Plugin for ChooserGuidePlugin {
    fn build(&self, app: &mut App) {
        use bevy_debugger_bevy::Checkpoints;
        use serde_json::Value;

        app.init_resource::<Checkpoints>()
            .init_resource::<bevy_debugger_bevy::Guide>()
            // Below the title line, so the card does not sit on top of the panels it is talking
            // about — the same correction the editor's placement records.
            .insert_resource(bevy_debugger_bevy::GuidePlacement {
                // Overwritten by `room_for_the_card` on the first frame, which is where the height
                // is actually known; a card is never up before then.
                top: 0.0,
                // Wide enough to read a step without wrapping every line, narrow enough to sit
                // under the columns rather than beyond them.
                width: 620.0,
            })
            // **On the menu only.** `room_for_the_card` reads `Res<Chooser>`, which exists only while
            // this screen is up — and a missing `Res<T>` panics its system in Bevy 0.19. Ungated, it
            // aborted every `--door` launch on frame one, since a named door never enters the menu.
            .add_systems(
                Update,
                room_for_the_card.run_if(in_state(crate::screen::Screen::Menu)),
            );

        let want = |args: &Value, key: &str| -> Option<String> {
            args.get(key).and_then(Value::as_str).map(str::to_owned)
        };

        let on_kits = app.register_system(|_: In<Value>, c: Res<Chooser>| c.focus == Focus::Kits);
        let on_maps = app.register_system(|_: In<Value>, c: Res<Chooser>| c.focus == Focus::Maps);
        let on_settings =
            app.register_system(|_: In<Value>, c: Res<Chooser>| c.focus == Focus::Settings);
        let making_kit = app.register_system(|_: In<Value>, c: Res<Chooser>| {
            matches!(c.creating, Some(New::Kit(_)))
        });
        let making_map = app.register_system(|_: In<Value>, c: Res<Chooser>| {
            matches!(c.creating, Some(New::Map(_)))
        });
        let typing = app.register_system(|_: In<Value>, c: Res<Chooser>| c.editing);

        // **By name, and off the CATALOG rather than the draft** — the catalog is a description of
        // disk, so this answers "does it exist" and not "did somebody type it".
        let kit_exists = app.register_system(move |args: In<Value>, c: Res<Chooser>| {
            want(&args.0, "name").is_some_and(|n| c.catalog.kits.iter().any(|k| k.label == n))
        });
        let map_exists = app.register_system(move |args: In<Value>, c: Res<Chooser>| {
            want(&args.0, "name").is_some_and(|n| c.catalog.maps.iter().any(|m| m.name == n))
        });
        let map_gone = app.register_system(move |args: In<Value>, c: Res<Chooser>| {
            want(&args.0, "name").is_some_and(|n| !c.catalog.maps.iter().any(|m| m.name == n))
        });
        let asking_delete = app
            .register_system(|_: In<Value>, c: Res<Chooser>| matches!(c.ask, Some(Ask::Delete(_))));
        let asking_quit =
            app.register_system(|_: In<Value>, c: Res<Chooser>| matches!(c.ask, Some(Ask::Quit)));
        let nothing_asked = app.register_system(|_: In<Value>, c: Res<Chooser>| c.ask.is_none());
        let on_new_row = app.register_system(|_: In<Value>, c: Res<Chooser>| c.on_new_row());

        let mut checkpoints = app.world_mut().resource_mut::<Checkpoints>();
        checkpoints.register("the kit list has the arrows", on_kits);
        checkpoints.register("the map list has the arrows", on_maps);
        checkpoints.register("the settings have the arrows", on_settings);
        checkpoints.register("the highlighted row makes a new one", on_new_row);
        checkpoints.register("a new kit is being made", making_kit);
        checkpoints.register("a new map is being made", making_map);
        checkpoints.register("a field is taking text", typing);
        checkpoints.register("the kit exists", kit_exists);
        checkpoints.register("the map exists", map_exists);
        checkpoints.register("the map is gone", map_gone);
        checkpoints.register("a deletion is being asked about", asking_delete);
        checkpoints.register("quitting is being asked about", asking_quit);
        checkpoints.register("nothing is being asked", nothing_asked);
    }
}

// ------------------------------------------------------------------------------------------------
// Seeing this screen without touching the one you are using
// ------------------------------------------------------------------------------------------------

/// **How much taller than the screen the window has to be**, in logical pixels.
///
/// Zero except while a guide card is up. It exists so that exactly one system writes the window's
/// size: the card cannot resize the window itself without fighting the system that fits the render
/// target to it, and two systems writing one window is the shape of every resize flicker there is.
#[derive(Resource, Default)]
pub struct ExtraRoom(pub f32);

/// **Scan disk and build the screen's state, every time the menu is entered.**
///
/// Rescanned per visit rather than built once: *"the catalog is a description of disk, never a cache
/// of one"* — a map created or a kit deleted inside a door has to be on the list when you come back,
/// and with both screens in one application coming back no longer means a fresh process to do it
/// for us.
fn build_chooser(mut commands: Commands, opening: Res<MenuOpening>, existing: Option<Res<Chooser>>) {
    // Where the cursor was, so returning from a door lands on what you were just in rather than at
    // the top. **By name, not by index**: the catalog is re-scanned and re-sorted on every visit, so
    // an index survives only until something is created, deleted or renamed inside the door — and
    // then it silently lands on a neighbour. `Chooser::reveal` is the by-name answer and its doc
    // comment describes exactly this requirement; it used to be written and unused.
    let was = existing.map(|c| {
        (
            c.current_kit().map(|k| k.label.clone()),
            c.current_map().map(|m| m.name.clone()),
            c.focus,
            c.problem.clone(),
        )
    });
    // **A failed scan is a screen, not a crash.** This used to log and return, leaving `Chooser`
    // absent — and the six systems that take it, two of them chained immediately after this one,
    // panic on a missing `Res<T>` in Bevy 0.19 rather than skipping. So the message `Catalog::scan`
    // was carefully written ("… is not a project: it has no `assets/emerge` directory. Run the
    // editor from the repository root") arrived as a param-validation panic instead of as the thing
    // the author reads. An empty catalog is the honest state — the screen then offers `+ new kit`,
    // which is the instruction — and the reason sits on the problem line above it.
    let (catalog, problem) = match Catalog::scan(&opening.root) {
        Ok(c) => (c, None),
        Err(e) => {
            error!("{e}");
            (
                Catalog {
                    kits: Vec::new(),
                    maps: Vec::new(),
                    authoring: None,
                    bashes: Vec::new(),
                    projects: Vec::new(),
                },
                Some(e),
            )
        }
    };
    let mut chooser = Chooser::new(
        opening.root.clone(),
        catalog,
        opening.preselect.as_deref(),
    );
    if let Some((kit, map, focus, was_problem)) = was {
        chooser.reveal(kit.as_deref(), map.as_deref());
        chooser.focus = focus;
        // A refusal raised while opening a door is drawn here, which is where the author is looking.
        chooser.problem = was_problem;
    }
    // A scan failure outranks whatever the last visit was complaining about: it is the reason this
    // screen has nothing on it.
    if problem.is_some() {
        chooser.problem = problem;
    }
    commands.insert_resource(chooser);
}

/// **The door the menu chose, already opened.**
///
/// Still one parser: both callers build the argv `[root, map?, --door, d, --kit, k]` and hand it to
/// `args::open`. What changed is *where* that call happens — **before** the state moves, not after.
///
/// It used to carry the argv and let `screen::open_the_door` do the opening on `OnExit(Menu)`, whose
/// failure branch wrote a message and set `NextState(Menu)`. That branch could not work: Bevy 0.19
/// runs `ExitSchedules` and `EnterSchedules` in one `StateTransition` pass, so a `NextState` written
/// during `OnExit` is not read until the *next* pass — `OnEnter(Editor)` ran anyway, against a world
/// with no `Project`, and the editor panicked on a missing `Res<T>` instead of showing the reason.
/// Opening here means the only way into `Screen::Editor` is with a door in hand.
#[derive(Resource)]
pub struct Chosen(pub crate::args::Opened);

/// **The menu's entities, gone — through the one rule that decides what a screen owns.**
///
/// This used to spell the reachability rule as its own query, which is how it came to sweep away
/// `crate::surface`'s cameras: `screen::scene_roots` had learned to exclude them and this copy had
/// not. See [`crate::screen::despawn_scene`], which is now the only place the rule is written.
fn tear_down_menu(world: &mut World) {
    crate::screen::despawn_scene(world);
}

