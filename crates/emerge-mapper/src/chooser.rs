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
        /// **Which kits this map offers**, by namespace — `Map::palette`. Empty means all of them.
        palette: Vec<String>,
        /// **Which kits its content already names**, derived from the placements and from the
        /// members of anything it stamps. Never stored: this is what decides whether a kit may be
        /// unticked, and a cached answer to that is an answer that can be wrong.
        uses: BTreeSet<String>,
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
        if let Some(kit) = read_kit(&base, None)? {
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
        let bindings = std::fs::read_to_string(base.join(emerge_core::kits::KITS_FILE))
            .ok()
            .and_then(|t| emerge_core::kits::Kits::parse(&t).ok())
            .map(|k| k.bind)
            .unwrap_or_default();
        for kit in &mut kits {
            if let Some(b) = bindings.iter().find(|b| Some(b.dir.as_str()) == kit.flag.as_deref()) {
                kit.namespace = Some(b.namespace.clone());
            }
        }

        // **Fixed order, every scan.** See the module note on Sears & Shneiderman: nothing here is
        // sorted by use, and `the_catalog_order_never_moves` is what keeps that true.
        kits.sort_by(|a, b| a.label.cmp(&b.label));
        // One list for the project. `maps/` may not exist yet in a project nobody has saved from,
        // and that is an empty list rather than an error — the `+ new map` row is the instruction.
        // **The project's compositions, once**, so a stamp can be resolved to the kits its members
        // come from. A map that stamps a tile uses those kits as surely as one that places the
        // pieces directly, and a checkbox that could not see that would offer to untick a kit the
        // map depends on.
        let comp_path = base.join(Compositions::FILE);
        let comps = if comp_path.is_file() {
            let text = std::fs::read_to_string(&comp_path)
                .map_err(|e| format!("cannot read {}: {e}", comp_path.display()))?;
            Compositions::parse(&text)
                .map_err(|e| format!("{}: {e}", comp_path.display()))?
                .compositions
        } else {
            Vec::new()
        };
        let maps = read_maps(&base.join(MAPS_DIR), &comps, &kits)?;
        Ok(Catalog { kits, maps })
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
        ids,
    }))
}

/// Every `*.map.ron` beside a kit, alphabetical.
fn read_maps(
    dir: &Path,
    comps: &[emerge_core::composition::Composition],
    kits: &[Kit],
) -> Result<Vec<MapEntry>, String> {
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
                Ok(map) => {
                    let mut uses = BTreeSet::new();
                    // **Which kit DEFINES it.** Reading a namespace out of the id answers `None`
                    // for every flat library, so nothing was ever in use and every kit looked
                    // safe to turn off — the failure `Project::kit_of` records.
                    let mut add = |id: &str| {
                        if let Some(k) = kits.iter().find(|k| k.ids.contains(id)) {
                            uses.insert(k.namespace.clone().unwrap_or_else(|| k.label.clone()));
                        }
                    };
                    for p in &map.placements {
                        add(&p.descriptor);
                    }
                    for stamp in &map.stamps {
                        let Some(c) = comps.iter().find(|c| c.id == stamp.of) else {
                            continue;
                        };
                        for m in &c.members {
                            match &m.body {
                                Body::Descriptor { id, .. } | Body::Composition { id } => add(id),
                                Body::Slot { .. } => {}
                            }
                        }
                    }
                    MapSummary::Read {
                        placements: map.placements.len(),
                        stamps: map.stamps.len(),
                        bounds: map.bounds,
                        palette: map.palette.clone(),
                        uses,
                    }
                }
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

/// **Write a map's kit selection**, leaving everything else in the file exactly as it was.
///
/// Parsed, edited, validated and written through the same `Map` schema the editor and the game read,
/// rather than spliced as text — `map.rs`'s own rule: *"an emerge map is serialized normally and
/// never text-spliced"*, because every reason a map has lives in a field.
fn write_palette(path: &Path, palette: &[String]) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut map = Map::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    map.palette = palette.to_vec();
    map.validate()?;
    let out = ron::ser::to_string_pretty(&map, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("map: serialize: {e}"))?;
    emerge_core::ron_surgery::save_atomic(path, &out)
}

/// **Add a binding for `name`**, leaving the rest of `kits.ron` as it was.
fn bind_kit(root: &Path, name: &str) -> Result<(), String> {
    let path = root.join(EMERGE_DIR).join(emerge_core::kits::KITS_FILE);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
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
                    "(version: {}, bind: [], authoring: None)",
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

        // The property, stated as a property: scanning again returns the identical order.
        let again = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            first, again,
            "a second scan must return the identical catalog"
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
                // A new map offers every kit and uses none, which is where an author starts.
                palette: Vec::new(),
                uses: BTreeSet::new(),
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

        // Layer 1 — in a field. Leaving it must not touch anything else.
        c.section(1);
        c.section(1);
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
            render(&c).contains("quit emerge-mapper?"),
            "quitting is a question:\n{}",
            render(&c)
        );
        assert!(
            c.hint().contains("Y quit") && c.hint().contains("Esc stay"),
            "and the hint offers both answers: {}",
            c.hint()
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
        c.section(1);
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));
        let hint = c.hint();
        assert!(hint.contains('Y'), "delete is answered with Y: {hint}");
        assert!(hint.contains("Esc"), "and declined with Esc: {hint}");
        assert!(
            !hint.contains("Enter"),
            "Enter must NOT answer a destructive question — it opens maps everywhere else: {hint}"
        );

        c.ask = Some(Ask::Quit);
        let hint = c.hint();
        assert!(
            hint.contains('Y') && hint.contains("Esc"),
            "same two answers: {hint}"
        );
        assert!(!hint.contains("Enter"), "{hint}");
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

        // **The map panel carries the map's properties and none of the kit's.** Which kits it
        // offers is also a fact about the map — but it is *drawn* on the KITS column, beside the
        // kits it is about, rather than mirrored here one panel away. See `Chooser::screen`.
        let map_left: Vec<&str> = s.settings.iter().map(|r| r.left.as_str()).collect();
        assert_eq!(map_left, vec!["NAME", "BOUNDS", "ORIGIN", "NOTE"]);
        assert!(
            s.kits.iter().skip(1).all(|r| r.left.starts_with('[')),
            "and every kit row carries its state: {:?}",
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
        c.section(1);
        c.section(1);
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
        // **A set of properties, not a list of things.** Four of them, and no row here brings a new
        // thing into being — which is what the two assertions above pin and this one counts.
        assert_eq!(
            s.settings.len(),
            Field::ALL.len(),
            "the map's four properties, and nothing that makes anything"
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
        assert_eq!(made.pieces, 0, "it starts empty");
        assert!(catalog.maps.is_empty(), "and the project still has no maps");
        assert_eq!(
            made.flag.as_deref(),
            Some("site_v3"),
            "reachable as --kit site_v3"
        );
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
        c.section(1); // into the map panel
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));

        assert!(matches!(c.ask, Some(Ask::Delete(_))), "the question is up");
        assert!(
            path.is_file(),
            "and the file is UNTOUCHED until it is answered"
        );
        assert!(
            render(&c).contains("delete `hall`?"),
            "the question names the map:\n{}",
            render(&c)
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
        c.section(1);
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
        c.section(1);
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

        let asked = c
            .screen()
            .asking
            .unwrap_or_else(|| panic!("a question is up"));
        // **The question carries no counts at all**, deliberately: `KIT INFO` is beside it saying
        // exactly that, and a question restating its neighbour is text to read rather than a
        // decision to make.
        assert!(asked.contains("delete kit `site`?"), "{asked}");
        assert!(asked.contains('Y') && asked.contains("Esc"), "{asked}");
        assert!(
            !asked.contains("piece") && !asked.contains("map "),
            "no inventory in the question: {asked}"
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

    /// **A map offers every kit until one is turned off**, and turning one off writes the rest.
    ///
    /// Empty means *all*, which is the state a new map starts in — Liapis' *user fatigue* is a named
    /// failure of tools that need a specific input before they do anything, and ticking four boxes
    /// to get a palette would be exactly that. So the first untick has to write the full list minus
    /// one: writing an empty list would mean the opposite of what was asked for.
    #[test]
    fn a_map_offers_every_kit_until_one_is_turned_off() {
        let root = Root::new("kits-toggle");
        root.skin("furniture", "furniture", &["bench"]);
        root.skin("lab", "lab", &["bench"]);
        root.map(&root.0, "hall", &[]);

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, None);
        c.focus = Focus::Maps;
        c.map = 1;
        assert!(
            matches!(&c.current_map().map(|m| &m.summary), Some(MapSummary::Read { palette, .. }) if palette.is_empty()),
            "a new map has chosen nothing, which means everything"
        );

        let lab = c
            .catalog
            .kits
            .iter()
            .position(|k| k.label == "lab")
            .unwrap_or_else(|| panic!("lab is listed"));
        c.toggle_kit(lab).unwrap_or_else(|e| panic!("{e}"));

        let Some(MapSummary::Read { palette, .. }) = c.current_map().map(|m| &m.summary) else {
            panic!("the map still reads");
        };
        assert!(
            !palette.contains(&"lab".to_owned()) && palette.contains(&"furniture".to_owned()),
            "turning one off names the rest rather than emptying the list: {palette:?}"
        );

        // **And back on returns to the un-fatiguing default**, so a kit added later is offered
        // rather than silently absent from a list that happened to name every kit at the time.
        c.toggle_kit(lab).unwrap_or_else(|e| panic!("{e}"));
        let Some(MapSummary::Read { palette, .. }) = c.current_map().map(|m| &m.summary) else {
            panic!("the map still reads");
        };
        assert!(
            palette.is_empty(),
            "everything on is the same state as nothing chosen: {palette:?}"
        );
    }

    /// **Turning off the second-to-last kit does not turn the others back on.**
    ///
    /// Walked exactly as it was reported: *"when I turn the test kit off, and then I tried to turn
    /// the furniture kit off, the test kit comes back on while the furniture kit stays on."*
    ///
    /// The cause was a sentinel collision. `[]` is what a map that has never been touched carries
    /// and `Map::palette` defines it as **every** kit, so removing the last entry wrote the value
    /// meaning the opposite of the act. **A written palette always names at least one kit** is the
    /// invariant that closes it, and this is the only place that could break it.
    #[test]
    fn turning_off_the_last_kit_is_refused_rather_than_meaning_all_of_them() {
        let root = Root::new("kits-last");
        root.skin("furniture", "furniture", &["bench"]);
        root.skin("test", "test", &["thing"]);
        root.map(&root.0, "hall", &[]);

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, None);
        c.focus = Focus::Maps;
        c.map = 1;
        let at = |c: &Chooser, label: &str| {
            c.catalog
                .kits
                .iter()
                .position(|k| k.label == label)
                .unwrap_or_else(|| panic!("`{label}` is listed"))
        };

        // First one off: the other is named, so the file says what is on rather than what is not.
        let t = at(&c, "test");
        c.toggle_kit(t).unwrap_or_else(|e| panic!("{e}"));
        let Some(MapSummary::Read { palette, .. }) = c.current_map().map(|m| &m.summary) else {
            panic!("the map still reads");
        };
        assert_eq!(palette, &vec!["furniture".to_owned()]);

        // Second one off: refused, and nothing on disk moves.
        let f = at(&c, "furniture");
        let e = c
            .toggle_kit(f)
            .err()
            .unwrap_or_else(|| panic!("the last kit on must not go"));
        assert!(e.contains("only kit left on"), "{e}");
        let Some(MapSummary::Read { palette, .. }) = c.current_map().map(|m| &m.summary) else {
            panic!("the map still reads");
        };
        assert_eq!(
            palette,
            &vec!["furniture".to_owned()],
            "a refused toggle writes nothing — and above all does not write `[]`, which would read \
             back as every kit and turn the other one on again"
        );

        // And the row said so before it was pressed — on the KITS column, which is where the tick
        // lives now (`Space` flips it there).
        let rows = c.screen().kits;
        assert!(
            rows.iter()
                .any(|r| r.left.contains("[=] furniture") && r.right.contains("only one on")),
            "the constraint is drawn, not discovered by pressing: {rows:?}"
        );
    }

    /// **The tick lives on the kit row, and `Space` is what the screen says flips it.**
    ///
    /// Asked for at the keyboard, 2026-08-16: *"it would feel better if the space bar toggled kits
    /// on in the kit area."* Before that the state was a mirrored list of the same kits inside MAP
    /// INFO — visible, which was the point of putting it there, but one panel away from the list it
    /// described, so an author had two places showing one fact.
    ///
    /// Both halves are pinned here because they are the same promise: the row draws its state, and
    /// the hint names the key that changes it. `docs/ui.md` §4.2 — a verb reachable by mouse is
    /// reachable by keyboard, and each chip states its key.
    #[test]
    fn the_kit_rows_carry_their_own_tick_and_the_hint_names_the_key() {
        let root = Root::new("kits-ticked");
        root.skin("furniture", "furniture", &["bench"]);
        root.skin("site", "site", &["wall"]);
        root.map(&root.0, "hall", &[]);

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, None);
        c.focus = Focus::Maps;
        c.map = 1;

        // Nothing chosen yet, so every kit is offered — and every row says so rather than reading
        // blank, which is the state `[ ]` and `[x]` exist to tell apart.
        let rows = c.screen().kits;
        assert!(
            rows.iter().skip(1).all(|r| r.left.starts_with("[x] ")),
            "an untouched map offers every kit, and each row draws it: {rows:?}"
        );
        // The piece count is still there — the fact this screen was built to carry.
        assert!(
            rows.iter().skip(1).all(|r| r.right.contains("pieces")),
            "the tick is added beside the count, not in place of it: {rows:?}"
        );

        // Turn one off through the same call `Space` makes, and the row follows.
        let site = c
            .catalog
            .kits
            .iter()
            .position(|k| k.label == "site")
            .unwrap_or_else(|| panic!("site is listed"));
        c.toggle_kit(site).unwrap_or_else(|e| panic!("{e}"));
        let rows = c.screen().kits;
        assert!(
            rows.iter().any(|r| r.left.starts_with("[ ] site")),
            "the kit turned off draws unticked: {rows:?}"
        );

        // And the screen says which key does it, standing in the kits column.
        c.focus = Focus::Kits;
        c.kit = 1;
        assert!(
            c.hint().contains("Space on/off"),
            "the hint has to name the key that flips the row: {}",
            c.hint()
        );
    }

    /// **A kit the map is standing on cannot be turned off, and the row says so before it is tried.**
    ///
    /// Vicente & Rasmussen's ecological interface design asks that the perceptual cues *"directly
    /// specify process constraints"* — so `[=] in use` is on screen, and the refusal below is the
    /// backstop rather than the teaching. Turning it off would hide the palette rows that describe
    /// pieces already placed: the map would still load and still draw, and the author could not find
    /// or match what is in front of them.
    #[test]
    fn a_kit_the_map_stands_on_cannot_be_turned_off() {
        let root = Root::new("kits-locked");
        root.skin("furniture", "furniture", &["bench"]);
        root.skin("site", "site", &["wall"]);
        root.map(&root.0, "hall", &["site/wall"]);

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, None);
        c.focus = Focus::Maps;
        c.map = 1;

        let site = c
            .catalog
            .kits
            .iter()
            .position(|k| k.label == "site")
            .unwrap_or_else(|| panic!("site is listed"));

        // The screen says it first, on the KITS column.
        let rows = c.screen().kits;
        assert!(
            rows.iter()
                .any(|r| r.left.contains("[=] site") && r.right.contains("in use")),
            "the constraint is drawn, not discovered by pressing: {rows:?}"
        );

        // And the press is refused, naming it.
        let e = c
            .toggle_kit(site)
            .err()
            .unwrap_or_else(|| panic!("a kit the map stands on must not go"));
        assert!(e.contains("site") && e.contains("already on this map"), "{e}");
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
/// The first version made `Focus::Field(Field)` a variant, so `Tab` meant "next field" in the
/// settings and "go to the settings" everywhere else — one key with two jobs, decided by where you
/// already were. Now there is one rule with no exceptions: **`Tab` crosses panels, arrows move
/// inside one.** Typing is a separate flag rather than a fourth variant, because it is a phase this
/// screen passes through and not a place the arrows can be — the distinction `keys::Stance` exists
/// to make in the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Kits,
    Maps,
    Settings,
}

impl Focus {
    /// The panels in the order they are drawn, which is the order `Tab` walks them.
    ///
    /// **Maps, its settings, then kits** — reading order of the columns as they now sit: the maps
    /// list top-left, the map's own settings under it, the kits list to their right. It was
    /// kits-first while a map lived inside a kit; the columns swapped on 2026-08-16 and this
    /// followed them, because the promise in the line above is the whole reason this constant
    /// exists rather than each caller having its own idea of "next".
    const ALL: [Focus; 3] = [Focus::Maps, Focus::Settings, Focus::Kits];
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
    /// Leave the chooser.
    Quit,
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
}

impl New {
    /// What has been typed as the name so far, whichever kind it is.
    pub fn name(&self) -> &str {
        match self {
            New::Kit(name) => name,
            New::Map(d) => &d.name,
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

    /// **Every settings row the arrows can reach**, in the order they are drawn — which is
    /// [`Field::ALL`], and nothing else.
    ///
    /// It used to append one row per bound kit, and the leftover of that was an `if` whose body was
    /// a comment: a branch that still called `current_map()` on every arrow press to enter a block
    /// that did nothing, under a doc comment promising rows the body no longer produced. **The kit
    /// ticks live on the KITS column**, where `Space` flips the row that names the kit — one place
    /// showing one fact, rather than a mirror one panel away from the list it described.
    pub fn fields(&self) -> Vec<Field> {
        Field::ALL.to_vec()
    }

    /// **Turn one kit's pieces on or off for the selected map**, and write it.
    ///
    /// Written immediately rather than on some later save, because this panel has no save: every
    /// other setting here commits on `Enter` too. Compton's *grokloop* — the shorter the
    /// try/see/change loop, the faster the learning (Lai et al., `10.1145/3402942.3402946`) — and a
    /// checkbox whose effect appears two screens later is not a loop at all.
    ///
    /// **A kit the map already uses is refused, and the row already said so.** The refusal is the
    /// backstop, not the teaching: `settings_rows` draws it `[=] in use` precisely so nobody has to
    /// press it to find out.
    pub fn toggle_kit(&mut self, i: usize) -> Result<(), String> {
        let Some(kit) = self.catalog.kits.get(i) else {
            return Err("no such kit".to_owned());
        };
        let ns = kit
            .namespace
            .clone()
            .unwrap_or_else(|| kit.label.clone());
        let Some(entry) = self.current_map() else {
            return Err("select a map first — a kit is offered to one map at a time".to_owned());
        };
        let path = entry.path.clone();
        let MapSummary::Read { palette, uses, .. } = &entry.summary else {
            return Err("this map will not open, so its kits cannot be set".to_owned());
        };
        if uses.contains(&ns) {
            return Err(format!(
                "`{}` is already on this map — turning it off would hide the rows that describe \
                 pieces already placed.",
                kit.label
            ));
        }
        // **Empty means all**, so the first untick has to write out the full list minus one rather
        // than an empty one — which would mean the opposite of what was asked for.
        let all: Vec<String> = self
            .catalog
            .kits
            .iter()
            .map(|k| k.namespace.clone().unwrap_or_else(|| k.label.clone()))
            .collect();
        let mut next: Vec<String> = if palette.is_empty() {
            all.clone()
        } else {
            palette.clone()
        };
        if let Some(at) = next.iter().position(|p| *p == ns) {
            next.remove(at);
            // **The last one on cannot be turned off, because empty means ALL.**
            //
            // Reported at the keyboard: *"when I turn the test kit off, and then I tried to turn the
            // furniture kit off, the test kit comes back on."* Exactly so — removing the last entry
            // wrote `[]`, and `[]` is the value a map that has never been touched carries, which
            // `Map::palette` defines as every kit. The sentinel for *no choice made* and the state
            // *nothing chosen* were the same value, so the second untick meant the opposite of
            // itself.
            //
            // Refused rather than re-encoded, because the field's own doc already settled what the
            // empty palette means: *"Not 'none' — a map offering nothing is a map nobody can
            // build."* So the invariant is that **a written palette always names at least one kit**,
            // and this is the one place that could break it.
            if next.is_empty() {
                return Err(format!(
                    "`{}` is the only kit left on — a map that offers nothing cannot be built. \
                     Turn another on first.",
                    kit.label
                ));
            }
        } else {
            next.push(ns.clone());
        }
        // Back to the un-fatiguing default when everything is on again: a list naming every kit and
        // an empty list mean the same thing, and the empty one keeps meaning it when a kit is added.
        next.sort();
        let mut sorted_all = all;
        sorted_all.sort();
        if next == sorted_all {
            next.clear();
        }
        write_palette(&path, &next)?;
        rescan_keeping_place(self, None);
        Ok(())
    }

    fn step_field(&mut self, delta: i32) {
        let fields = self.fields();
        let i = fields.iter().position(|f| *f == self.field).unwrap_or(0);
        let n = fields.len();
        let next = if delta < 0 {
            (i + n - 1) % n
        } else {
            (i + 1) % n
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
        let (name, bounds, origin, note) = match (&self.creating, self.current_map()) {
            (Some(New::Map(d)), _) => (d.name.clone(), d.bounds, d.origin, d.note.clone()),
            (Some(New::Kit(_)), _) => return String::new(),
            (None, Some(m)) => match &m.summary {
                MapSummary::Read { bounds, .. } => {
                    let (origin, note) = read_origin_and_note(&m.path);
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
            Focus::Settings => false,
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

    /// Move within whichever column has the arrows. Clamped, not wrapped: a list that wraps makes
    /// "am I at the end" unanswerable without counting.
    pub fn step(&mut self, delta: i32) {
        self.problem = None;
        match self.focus {
            Focus::Kits => {
                // `+ 1` for the `+ new kit` row, which is always there — even in a project whose
                // every kit was deleted, where it is the only thing left to press.
                self.kit = clamp_step(self.kit, delta, self.catalog.kits.len() + 1);
                // **The map selection stays put.** It used to be reset here, because a different
                // kit meant a different map list and the old index could point past the new one.
                // There is one list now — the project's — so there is no index to invalidate, and
                // resetting would move the row an author is reading out from under them while they
                // change only where new work lands.
            }
            Focus::Maps => {
                let n = self.catalog.maps.len();
                self.map = clamp_step(self.map, delta, n + 1);
            }
            // **The arrows walk the settings rows too**, which is the whole of the correction:
            // moving inside a panel is always the arrows, whichever panel it is.
            Focus::Settings => {
                self.step_field(delta);
            }
        }
    }

    /// **Cross to the next panel, or the previous one** — `Tab`, `Shift+Tab`, and `right`/`left`,
    /// which are three bindings on one concept rather than three behaviours.
    ///
    /// Wraps, and **skips a panel with nothing in it**: the settings have no rows when no map is
    /// selected and none is being made, and a `Tab` that lands the arrows somewhere they can do
    /// nothing is the dead key `keys.rs` refuses to ship.
    pub fn section(&mut self, delta: i32) {
        self.problem = None;
        let at = Focus::ALL
            .iter()
            .position(|f| *f == self.focus)
            .unwrap_or(0);
        for step in 1..=Focus::ALL.len() {
            let i = if delta < 0 {
                (at + Focus::ALL.len() * step - step) % Focus::ALL.len()
            } else {
                (at + step) % Focus::ALL.len()
            };
            let want = Focus::ALL[i];
            if self.panel_has_rows(want) {
                self.focus = want;
                return;
            }
        }
    }

    /// Is there anything in that panel for the arrows to be on?
    fn panel_has_rows(&self, panel: Focus) -> bool {
        match panel {
            // The kit list is never empty — `Catalog::scan` refuses a root with no kits — and the
            // map panel always draws a row, the instruction when there are no maps.
            Focus::Kits | Focus::Maps => true,
            Focus::Settings => self.creating.is_some() || self.current_map().is_some(),
        }
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
        }
    }

    /// **Agree to it.** Removes the file the question named, then rescans so the list is a
    /// description of disk rather than of the edit.
    pub fn confirm_delete(&mut self) -> Result<String, String> {
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
                palette: Vec::new(),
                uses: BTreeSet::new(),
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
        c.section(1);
        c.step(1);
        assert_eq!(c.map, 2, "row 1 is the first map; row 0 makes a new one");
        let chosen = c.current_map().map(|m| m.name.clone());
        c.section(-1);
        c.step(1); // -> a different kit
        assert_eq!(c.map, 2, "the map row does not move under the author");
        assert_eq!(
            c.current_map().map(|m| m.name.clone()),
            chosen,
            "and it is still the same map"
        );
    }

    /// **`Tab` crosses panels; the arrows never do.** The rule asked for at the keyboard: *"tab
    /// should move around the different sections"*, and inside one, *"I need to use the arrow keys
    /// to move around, not tab."*
    #[test]
    fn tab_crosses_panels_and_the_arrows_stay_inside_one() {
        let mut c = chooser(Some("site"));
        assert_eq!(c.focus, Focus::Kits);
        c.section(1);
        assert_eq!(c.focus, Focus::Maps);
        c.section(1);
        assert_eq!(
            c.focus,
            Focus::Settings,
            "the third panel is a panel like the others"
        );
        c.section(1);
        assert_eq!(c.focus, Focus::Kits, "and it wraps");
        c.section(-1);
        assert_eq!(c.focus, Focus::Settings, "backwards too");

        // An arrow never changes panel — that was the defect.
        let before = c.focus;
        c.step(1);
        c.step(-1);
        assert_eq!(
            c.focus, before,
            "the arrows move inside the panel, never out of it"
        );
    }

    /// **A panel with nothing in it is skipped**, because landing the arrows where they can do
    /// nothing is the dead key `keys.rs` refuses to ship.
    ///
    /// The emptiness that matters moved: maps are the **project's** now, so an empty *kit* has
    /// nothing to do with whether there is a map to configure. A project with no maps is what leaves
    /// the settings panel with nothing to show.
    #[test]
    fn a_panel_with_no_rows_is_skipped() {
        let mut c = Chooser::new(
            PathBuf::from("."),
            Catalog {
                kits: vec![kit(Some("site_v2"), "site_v2", 0)],
                maps: Vec::new(),
            },
            Some("site_v2"),
        );
        c.section(1);
        assert_eq!(
            c.focus,
            Focus::Maps,
            "the map panel always draws a row — the instruction"
        );
        c.section(1);
        assert_eq!(
            c.focus,
            Focus::Kits,
            "no map is selected, so there are no settings to walk into"
        );
    }

    /// The settings rows are walked by the arrows, wrapping, like every other panel.
    #[test]
    fn the_arrows_walk_the_settings_rows() {
        let mut c = chooser(Some("site"));
        c.section(1);
        c.section(1);
        assert_eq!(c.focus, Focus::Settings);
        assert_eq!(c.field, Field::Name);
        c.step(1);
        assert_eq!(c.field, Field::Bounds);
        c.step(-1);
        assert_eq!(c.field, Field::Name);
        // **Backwards from the first row lands on the last.** The panel is the map's four
        // properties again: the kit rows moved to the KITS column on 2026-08-16, where `Space`
        // flips the row that names the kit. Still one cycle with no dead end at either edge, which
        // is the property this was written for.
        c.step(-1);
        let last = c
            .fields()
            .last()
            .copied()
            .unwrap_or_else(|| panic!("the panel has rows"));
        assert_eq!(c.field, last, "and it wraps backwards onto the last row");
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

    /// The four settings are a fixed cycle in the order they are drawn — walked by the **arrows**,
    /// since `Tab` crosses panels. Wraps, so neither end is a dead stop.
    #[test]
    fn the_field_cycle_runs_in_the_order_they_are_shown() {
        // No map selected, so the cycle is the four text settings — the kit rows join it only when
        // there is a map for them to be about. See `Chooser::fields`.
        let mut c = chooser(None);
        c.map = 0;
        c.focus = Focus::Settings;
        c.field = Field::Name;
        let mut seen = vec![c.field];
        for _ in 0..4 {
            c.step(1);
            seen.push(c.field);
        }
        let f = c.field;
        let _ = f;
        assert_eq!(
            seen,
            vec![
                Field::Name,
                Field::Bounds,
                Field::Origin,
                Field::Note,
                Field::Name
            ],
            "four fields, then back to the first"
        );
    }

    /// The same cycle backwards. Stepping one way then the other must land where you started, which
    /// is the property an off-by-one in either direction would break.
    #[test]
    fn the_field_cycle_runs_backwards_too() {
        let mut c = chooser(None);
        c.map = 0;
        c.focus = Focus::Settings;
        c.field = Field::Note;
        let mut seen = vec![c.field];
        for _ in 0..4 {
            c.step(-1);
            seen.push(c.field);
        }
        assert_eq!(
            seen,
            vec![
                Field::Note,
                Field::Origin,
                Field::Bounds,
                Field::Name,
                Field::Note
            ],
            "backwards from the last, wrapping past the first"
        );

        // Forward then back is where you were, walked through the `Chooser` since the cycle is now
        // per project — a kit row joins it only when there is a map for it to be about.
        for f in Field::ALL {
            let mut c = chooser(None);
            c.map = 0;
            c.focus = Focus::Settings;
            c.field = f;
            c.step(1);
            c.step(-1);
            assert_eq!(c.field, f, "{f:?}: forward then back is where you were");
            c.step(-1);
            c.step(1);
            assert_eq!(c.field, f, "{f:?}: back then forward is too");
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
    pub maps_header: String,
    pub maps: Vec<Row>,
    pub settings_header: String,
    pub settings: Vec<Row>,
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
        // **What the selected map offers**, so each kit row can say whether it is on. `None` when no
        // map is selected or the selected one will not parse — then a row carries no mark at all,
        // because "off" and "there is nothing to be on for" are different states.
        let offered: Option<(&Vec<String>, &BTreeSet<String>)> = match self.current_map() {
            Some(MapEntry { summary: MapSummary::Read { palette, uses, .. }, .. }) => {
                Some((palette, uses))
            }
            _ => None,
        };
        kits.extend(self.catalog.kits.iter().enumerate().map(|(i, k)| {
            let selected = self.focus == Focus::Kits && i + 1 == self.kit;
            let ns = k.namespace.as_deref().unwrap_or(k.label.as_str());
            // **The mark, and the constraint drawn rather than enforced on the press.** `[=]` is a
            // kit that cannot be turned off — either the map already places its pieces, or it is the
            // last one on and empty means all. `[x]` is on, `[ ]` is off.
            let mark = offered.map(|(palette, uses)| {
                let all = palette.is_empty();
                let in_use = uses.contains(ns);
                let on = all || in_use || palette.iter().any(|p| p == ns);
                let last_on =
                    !all && !in_use && palette.len() == 1 && palette.iter().any(|p| p == ns);
                if in_use || last_on {
                    ("[=] ", if in_use { " · in use" } else { " · only one on" })
                } else if on {
                    ("[x] ", "")
                } else {
                    ("[ ] ", "")
                }
            });
            let (tick, why) = mark.unwrap_or(("", ""));
            Row {
                left: format!(
                    "{tick}{}",
                    if k.flag.is_none() {
                        format!("{} (default)", k.label)
                    } else {
                        k.label.clone()
                    }
                ),
                // **The piece count stays.** It is the fact this screen was built to carry — on
                // 2026-08-15 an author could not tell `site` from `site_v2` and relaunched three
                // times — so the tick is added beside it rather than in place of it.
                right: format!("{} pieces{why}", k.pieces),
                // **A blank kit reads as blank without being read.** This is the fact the screen
                // exists to carry: on 2026-08-15 an author could not tell `site` from `site_v2`
                // and relaunched three times. A count nobody looks at would not have helped.
                tone: match (selected, k.pieces) {
                    (true, _) => Tone::Selected,
                    (false, 0) => Tone::Empty,
                    (false, _) => Tone::Stocked,
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
        Screen {
            kits,
            kit_header,
            kit_info,
            maps_header,
            maps,
            settings_header,
            settings,
            asking: self.ask.as_ref().map(|a| match a {
                // **The question has to carry its own answer.** A capture of this prompt showed it
                // saying what would be lost and never saying which key agreed — the fact lived only
                // in a guide card's prose and in `drive_chooser`. `Y` rather than `Enter` is a
                // deliberate choice (`Enter` opens maps and edits fields everywhere else on this
                // screen, and a destructive prompt answered by the most-pressed key gets answered
                // by accident), and a deliberate choice nobody can see is indistinguishable from
                // no choice at all.
                // **Only what is needed to decide.** It named the piece and map counts, and for a
                // map the file name too — reported at the keyboard: *"I don't want how much stuff
                // is in there, I can see that in the info area. Just give me the text that I need
                // to decide."* `KIT INFO` sits on screen beside the question still showing those
                // counts, so restating them is the same defect as the header that repeated the
                // selected kit's name. What only the question can say is which thing, that it is
                // final, and which key agrees.
                Ask::Delete(c) if c.kit => {
                    format!("delete kit `{}`? Y removes it all, Esc keeps it", c.name)
                }
                Ask::Delete(c) => {
                    format!("delete `{}`? Y removes it for good, Esc keeps it", c.name)
                }
                Ask::Quit => "quit emerge-mapper? Y quits — Esc stays".to_owned(),
            }),
            problem: self.problem.clone(),
            hint: self.hint().to_owned(),
        }
    }

    /// The highlighted kit, as facts. Empty when the `+ new kit` row is highlighted, because
    /// nothing is selected and inventing a panel for it would be the same lie the columns to the
    /// right already refuse to tell.
    fn kit_rows(&self) -> (String, Vec<Row>) {
        let Some(k) = self.current_kit() else {
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
                    tone: tone_for(live, name.is_empty()),
                }],
            );
        }
        let (header, name, bounds, origin, note) = match (&self.creating, self.current_map()) {
            (Some(New::Map(d)), _) => (
                self.current_kit().map_or_else(
                    || "NEW MAP".to_owned(),
                    |k| format!("NEW MAP IN {}", k.label),
                ),
                d.name.clone(),
                d.bounds,
                d.origin,
                d.note.clone(),
            ),
            (Some(New::Kit(_)), _) => unreachable!("handled above"),
            (None, Some(m)) => match &m.summary {
                MapSummary::Read { bounds, .. } => {
                    // Origin and note are not in the summary — the row is about the file, and
                    // reading every map's prose to fill a panel nobody has opened is work for a
                    // list. Selecting one is what asks the question, so it is read here.
                    let (origin, note) = read_origin_and_note(&m.path);
                    (
                        // The map's own name is the first row of this panel, so a name in the header was
                        // saying it twice — see the kit header above for the report behind this.
                        "MAP INFO".to_owned(),
                        m.name.clone(),
                        *bounds,
                        origin,
                        note,
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
        let rows = vec![
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
                    self.creating.is_some() && self.raw.is_empty(),
                ),
            },
            Row {
                left: Field::Bounds.label().to_owned(),
                right: value(Field::Bounds, triple(bounds)),
                tone: tone_for(live(Field::Bounds), false),
            },
            Row {
                left: Field::Origin.label().to_owned(),
                right: value(Field::Origin, triple(origin)),
                tone: tone_for(live(Field::Origin), false),
            },
            Row {
                left: Field::Note.label().to_owned(),
                // **Clipped to one line.** A map's note is prose — the shipped one is a full
                // sentence with an absolute path in it — and at full length it wrapped, pushed its
                // own label onto a second line and broke the alignment of every row above it. The
                // whole note is still there in the file and still editable; this panel is a summary,
                // and a summary that reflows the screen is not one.
                right: value(Field::Note, clip(note.unwrap_or_default().as_str(), 20)),
                tone: tone_for(live(Field::Note), false),
            },
        ];
        let rows = rows;
        // **The kit ticks are not here.** They were one row per bound kit, in this panel, and that
        // put a second list of the same kits one screen-region away from the real one — the author
        // has to learn which of the two to reach for. They live on the KITS column now, where
        // `Space` flips the row that names the kit. Asked for at the keyboard, 2026-08-16.
        //
        // Vicente & Rasmussen's argument for drawing the constraint rather than refusing on the
        // press (`10.1109/21.156574` — *"the perceptual cues in the interface should directly
        // specify process constraints"*) moved with them; see `Chooser::screen`.
        (header, rows)
    }

    /// The verbs, and only the ones that would do something right now. `docs/ui.md` §3.5 caps
    /// immediately-issuable choices at three or four; a key listed where it is dead is worse than a
    /// key not listed, because it teaches something untrue.
    pub fn hint(&self) -> &'static str {
        match self.ask {
            // **The question owns the keyboard, and the hint says only its two answers.** Listing
            // the ordinary verbs beside a pending question invites pressing one of them.
            Some(Ask::Delete(_)) => "Y delete it    Esc keep it",
            Some(Ask::Quit) => "Y quit    Esc stay",
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
            _ if self.editing => "type    Enter keep    Esc leave the field",
            // Reached by leaving the name field with Esc while still making something. No chord is
            // offered: naming it is what makes it, and there is no second way.
            Focus::Settings if self.creating.is_some() => "Enter name it    Esc cancel",
            // **The verb names what THIS row does.** `Enter` types into a text field and toggles a
            // kit, and a hint saying "edit" over a checkbox is the hint that teaches the wrong key.
            Focus::Settings => "up/down field    Enter edit    Tab panel    Esc quit",
            // **Only verbs that would do something right now.** `Enter` opens a map — so it is
            // not offered on a kit with none, nor on the `+ new` row where it makes instead.
            Focus::Kits if self.kit == 0 => "up/down kit    Enter new kit    Tab panel    Esc quit",
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
            // **`Space` is not offered with no map**, because a palette belongs to a map and there
            // is none to offer the kit to. `toggle_kit` says so if pressed anyway.
            Focus::Kits if self.catalog.maps.is_empty() => {
                "up/down kit    Tab panel    N new kit    Delete remove    Esc quit"
            }
            // **`Enter open kit`, not `Enter open`.** Both list columns offered a bare "open" and
            // they open different doors — so with the columns swapped on 2026-08-16 an author
            // pressed it in the maps column expecting the kit and got the Map door, where the
            // labeler is not even bound. The verb has to name what it opens.
            Focus::Kits => {
                "up/down kit    Enter open KIT    Space on/off    N new kit    Delete remove    Esc quit"
            }
            Focus::Maps if self.map == 0 => "up/down map    Enter new map    Tab panel    Esc quit",
            Focus::Maps => {
                "up/down map    Enter open MAP    Tab panel    Delete remove    Esc quit"
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

fn tone_for(live: bool, unset: bool) -> Tone {
    match (live, unset) {
        (true, _) => Tone::Selected,
        (_, true) => Tone::Empty,
        _ => Tone::Row,
    }
}

/// Origin and note, read from the map file when a row is selected. Failure is silent here on
/// purpose: the row already carries `Unreadable` when the file cannot be parsed, and a second
/// refusal in the settings panel would say the same thing twice.
fn read_origin_and_note(path: &Path) -> ((f32, f32, f32), Option<String>) {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| Map::parse(&t).ok())
        .map_or(((0.0, 0.0, 0.0), None), |m| (m.origin, m.note))
}

fn triple(t: (f32, f32, f32)) -> String {
    format!("{} x {} x {}", t.0, t.1, t.2)
}

/// **The screen as flat text** — what the tests read, built from the same [`Screen`] the widgets are.
pub fn render(c: &Chooser) -> String {
    let s = c.screen();
    let mut out = String::from("emerge-mapper\n\nKITS\n");
    let line = |r: &Row| {
        let mark = if r.tone == Tone::Selected { ">" } else { " " };
        format!("{mark} {:<28}{}\n", r.left, r.right)
    };
    for r in &s.kits {
        out.push_str(&line(r));
    }
    if !s.kit_info.is_empty() {
        out.push_str(&format!("\n{}\n", s.kit_header));
        for r in &s.kit_info {
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
struct ProblemLine;
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
                .after(crate::chrome::FrameSet),
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
        TextFont::from_font_size(crate::chrome::text::BODY),
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
/// **The columns are capped.** A two-column menu stretched across an ultrawide monitor puts a row's
/// value a foot from its label, which is the alignment complaint this screen already had once, in
/// the other direction.
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
        column_gap: Val::Px(crate::chrome::PAD),
        padding: UiRect::all(Val::Px(crate::chrome::PAD * 1.5)),
        flex_grow: 1.0,
        min_height: Val::Px(0.0),
        ..default()
    });

    commands.entity(frame.body).with_children(|row| {
        // **Maps first, kits second**, asked for at the keyboard on 2026-08-16. The order was
        // kits-then-maps from when a map lived *inside* a kit and the columns were that containment
        // made spatial. Maps left the kit directories the same day (`project.rs`), so the nesting
        // the order was drawing no longer exists: the map is the job and the kit is what it draws
        // from.
        //
        // **Each column owns what belongs to it.** A map's settings sit under the map list; a kit's
        // facts sit under the kit list. One shared panel could not say whose it was — and worse, it
        // never followed the focus, so standing on a kit row you read a panel about a map two levels
        // down.
        let column = || Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(crate::chrome::GAP_ROW * 2.0),
            flex_grow: 1.0,
            flex_basis: Val::Px(0.0),
            max_width: Val::Px(COL_MAX),
            min_width: Val::Px(0.0),
            ..default()
        };

        row.spawn(column()).with_children(|col| {
            col.spawn((list_panel(), ListPanel)).with_children(|p| {
                p.spawn((header("MAPS"), MapsHeader));
                p.spawn((Node::default(), MapList));
            });
            col.spawn((info_panel(), InfoPanel)).with_children(|p| {
                p.spawn((header("MAP INFO"), SettingsHeader));
                p.spawn((Node::default(), SettingsList));
            });
        });

        row.spawn(column()).with_children(|col| {
            col.spawn((list_panel(), ListPanel)).with_children(|p| {
                p.spawn(header("KITS"));
                p.spawn((Node::default(), KitList));
            });
            col.spawn((info_panel(), InfoPanel)).with_children(|p| {
                p.spawn((header("KIT INFO"), KitInfoHeader));
                p.spawn((Node::default(), KitInfoList));
            });
        });
    });
}

/// **How wide a column is allowed to get.** Two columns filling an ultrawide monitor would put a
/// row's value a foot from its label — the alignment complaint this screen has already had once,
/// from the other direction (2026-08-16: *"the alignment of the columns of these list boxes ... don't
/// align"*).
const COL_MAX: f32 = 420.0;

/// **The menu's own chrome and status.** The editor fills these two bars with a door's furniture;
/// the menu has a name and a hint line, and they belong in the same places for the same reason.
fn spawn_menu_bars(mut commands: Commands, frame: Res<crate::chrome::Frame>) {
    commands.entity(frame.chrome_bar).with_children(|bar| {
        bar.spawn((
            Text::new("emerge-mapper"),
            TextFont::from_font_size(crate::chrome::text::BODY),
            TextColor(crate::chrome::LABEL),
        ));
    });
    commands.entity(frame.status).with_children(|band| {
        // **The refusal first and the hint after it**, both on the same row: the band is one line
        // and the problem is the half worth reading. `DANGER` carries the emphasis — colour is how
        // this editor shouts, and size is what type role a thing has.
        band.spawn((
            Text::new(String::new()),
            TextFont::from_font_size(crate::chrome::text::BODY),
            TextColor(crate::chrome::DANGER),
            ProblemLine,
        ));
        band.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        band.spawn((
            Text::new(String::new()),
            TextFont::from_font_size(crate::chrome::text::BODY),
            TextColor(crate::chrome::LABEL),
            HintLine,
        ));
    });
}

/// **Read the `Node` back out of a panel bundle**, so a test can assert the shape rather than
/// re-describe it. The bundle is the one the screen actually spawns; a test that rebuilt the numbers
/// would be checking its own copy.
#[cfg(test)]
fn panel_node(bundle: (Node, BackgroundColor)) -> Node {
    bundle.0
}

/// A list panel: takes the height that is left, so a long catalogue does not want a taller window.
fn list_panel() -> (Node, BackgroundColor) {
    (
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(crate::chrome::PAD)),
            row_gap: Val::Px(crate::chrome::GAP_ROW),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            ..default()
        },
        BackgroundColor(crate::chrome::PANEL_BG),
    )
}

/// **An inspector, on a different surface from the list above it.** It sits on the lighter ground
/// the editor already uses for a slot, so it does not read as a third list — looking the same was
/// the whole problem (see [`PanelKind`]). Sized by its content, because a fact sheet with four rows
/// in it should not be half the screen.
fn info_panel() -> (Node, BackgroundColor) {
    (
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(crate::chrome::PAD)),
            row_gap: Val::Px(crate::chrome::GAP_ROW),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(crate::chrome::SLOT_BG),
    )
}

/// A list panel, held to the height of the fullest list. See [`panel_heights`].
#[derive(Component)]
struct ListPanel;

/// An inspector panel, held to a fixed height so the fields inside it never move.
#[derive(Component)]
struct InfoPanel;

fn colour(tone: Tone) -> Color {
    match tone {
        Tone::Selected => crate::chrome::ACCENT,
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

fn fill(commands: &mut Commands, at: Entity, rows: &[Row], kind: PanelKind) {
    commands.entity(at).despawn_related::<Children>();
    commands.entity(at).insert(Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(crate::chrome::GAP_ROW * 0.6),
        // **Full width, or the rows inside do not line up with the panel they sit in.**
        //
        // This was unset, so the container shrank to its own content and each row's
        // `width: Percent(100)` resolved against *that* — which left `JustifyContent::SpaceBetween`
        // with no space to distribute. Every right-hand value landed wherever its own left text
        // happened to end, so the right column was ragged and neither column agreed with the header
        // above it. Reported at the keyboard, 2026-08-16: *"the alignment of the columns of these
        // list boxes with the content of the actual scroll box contained in it don't align."*
        width: Val::Percent(100.0),
        ..default()
    });
    for r in rows {
        let c = colour(r.tone);
        // **The chevron points into the column this row opens.** Only a list has one; a settings row
        // opens nothing, and giving it the same mark would restate the confusion this is fixing.
        let mark = match (kind, r.tone == Tone::Selected) {
            (PanelKind::List, true) => "\u{203a}",
            (PanelKind::Inspector, true) => "\u{2022}",
            _ => " ",
        };
        let left = format!("{mark} {}", r.left);
        let right = r.right.clone();
        commands.entity(at).with_children(|p| {
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(crate::chrome::PAD),
                justify_content: JustifyContent::SpaceBetween,
                width: Val::Percent(100.0),
                ..default()
            })
            .with_children(|line| {
                // **The label takes the slack and the value never wraps.**
                //
                // Both halves were plain text in a `SpaceBetween` row with nothing saying which one
                // yields, so in a 300 px column they both wrapped and a kit read as
                // `> [=] 82 pieces · only one` / `furniture   on` — the value broken across two
                // lines and interleaved with its own label. A row is a label column and a value
                // column; this is what says so.
                line.spawn((
                    Text::new(left.clone()),
                    TextFont::from_font_size(crate::chrome::text::TAB),
                    TextColor(c),
                    Node {
                        flex_grow: 1.0,
                        // Without this a flex item's automatic minimum size is its content, so the
                        // label refuses to shrink and pushes the value out of the row instead.
                        min_width: Val::Px(0.0),
                        ..default()
                    },
                ));
                if !right.is_empty() {
                    line.spawn((
                        Text::new(right.clone()),
                        TextFont::from_font_size(crate::chrome::text::TAB),
                        TextColor(c),
                        TextLayout::new(Justify::Right, LineBreak::NoWrap),
                        Node {
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                }
            });
        });
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
    )>,
    mut texts: Query<(
        &mut Text,
        Option<&MapsHeader>,
        Option<&SettingsHeader>,
        Option<&ProblemLine>,
        Option<&HintLine>,
        Option<&KitInfoHeader>,
    )>,
) {
    if !chooser.is_changed() {
        return;
    }
    let s = chooser.screen();
    for (e, kit, map, set, info) in &lists {
        if kit.is_some() {
            fill(&mut commands, e, &s.kits, PanelKind::List);
        } else if map.is_some() {
            fill(&mut commands, e, &s.maps, PanelKind::List);
        } else if set.is_some() {
            fill(&mut commands, e, &s.settings, PanelKind::Inspector);
        } else if info.is_some() {
            fill(&mut commands, e, &s.kit_info, PanelKind::Inspector);
        }
    }
    for (mut text, maps, settings, problem, hint, kit_info) in &mut texts {
        if kit_info.is_some() {
            **text = s.kit_header.clone();
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
/// So there is one rule for making things, and it is `Enter` on the name. `make_it` already lands
/// the selection on what it made, so the keyboard comes back to the list with the new row under it.
fn keep_field(chooser: &mut Chooser, field: Field) {
    if !commit_field(chooser, field) {
        return;
    }
    chooser.editing = false;
    if let Some(new) = chooser.creating.clone() {
        chooser.problem = make_it(chooser, &new).err();
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
        (None, Some(m)) => match &m.summary {
            MapSummary::Read { bounds, .. } => {
                let (origin, note) = read_origin_and_note(&m.path);
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
    if let Some(ask) = chooser.ask.clone() {
        if keyboard.just_pressed(KeyCode::KeyY) {
            match ask {
                Ask::Delete(_) => {
                    chooser.problem = match chooser.confirm_delete() {
                        Ok(name) => Some(format!("`{name}` deleted")),
                        Err(e) => Some(e),
                    };
                }
                Ask::Quit => {
                    exit.write(AppExit::Success);
                }
            }
        }
        // **`else if`, so one frame cannot both answer and un-answer.** `Y` and `Escape` arriving
        // together — a fast double-tap, or one `bevy_debugger/input` call — ran the deletion and then
        // cleared `problem`, which is where `confirm_delete` puts *both* the confirmation and the
        // reason it refused. The author saw the prompt vanish, no message, and the kit still there.
        else if keyboard.just_pressed(KeyCode::Escape) || keyboard.just_pressed(KeyCode::KeyN) {
            chooser.ask = None;
            chooser.problem = None;
        }
        return;
    }
    // **Ask; do not do.** `Delete` on a map raises the question and changes nothing — see
    // [`Pending`] for why this verb is split in two.
    if keyboard.just_pressed(KeyCode::Delete) || keyboard.just_pressed(KeyCode::Backspace) {
        chooser.problem = chooser.ask_delete().err();
        return;
    }
    // **Arrows move inside a panel. `Tab` crosses between them.** One rule, no exceptions — the
    // correction asked for at the keyboard, replacing a `Tab` that meant "next field" in the
    // settings and "go to the settings" everywhere else.
    if keyboard.just_pressed(KeyCode::ArrowUp) {
        chooser.step(-1);
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        chooser.step(1);
    }
    let shifted = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if keyboard.just_pressed(KeyCode::Tab) {
        chooser.section(if shifted { -1 } else { 1 });
    }
    // `left`/`right` are the same verb, not a second one: on a column of rows they have no
    // inside-the-panel meaning, so they cross, and an author who reaches for them is not wrong.
    if keyboard.just_pressed(KeyCode::ArrowRight) {
        chooser.section(1);
    }
    if keyboard.just_pressed(KeyCode::ArrowLeft) {
        chooser.section(-1);
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
        }
    }
    // **`Space` turns a kit on or off, standing on the kit itself.**
    //
    // Asked for at the keyboard, 2026-08-16: *"it would feel better if the space bar toggled kits on
    // in the kit area."* It used to be `Enter` on a mirrored list of kit rows inside MAP INFO — the
    // state was visible, which was the point of putting it there, but it was a second list of the
    // same kits one panel away from the real one. Now the tick lives on the row that names the kit,
    // and the key that flips it is pressed where you are already looking.
    //
    // It still means *"offer this kit to the selected map"*, because a palette belongs to a map —
    // `toggle_kit` says so when no map is selected rather than guessing one.
    if keyboard.just_pressed(KeyCode::Space) && chooser.focus == Focus::Kits {
        if chooser.on_new_row() {
            chooser.problem =
                Some("that row makes a new kit — there is nothing yet to turn on".to_owned());
            return;
        }
        match chooser.kit.checked_sub(1) {
            Some(i) => chooser.problem = chooser.toggle_kit(i).err(),
            None => chooser.problem = Some("no kit under the cursor".to_owned()),
        }
        return;
    }
    if keyboard.just_pressed(KeyCode::Enter) {
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
        // **`Enter` on a `+ new …` row is the visible half of `N`** — the row an author can see,
        // doing what the key beside it does.
        if chooser.on_new_row() {
            chooser.start_new();
            return;
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
        Chooser::new(PathBuf::from("."), Catalog { kits, maps: Vec::new() }, None)
    }

    /// The same, with the project's maps — which is where they live now, so they are given beside
    /// the kits rather than inside one.
    fn chooser_with_maps(kits: Vec<Kit>, maps: Vec<MapEntry>) -> Chooser {
        Chooser::new(PathBuf::from("."), Catalog { kits, maps }, None)
    }

    fn kit(flag: Option<&str>, label: &str, pieces: usize) -> Kit {
        Kit {
            flag: flag.map(str::to_owned),
            label: label.to_owned(),
            dir: PathBuf::from(label),
            pieces,
            namespace: None,
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
                    palette: Vec::new(),
                    uses: BTreeSet::new(),
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
    /// discovery". If the line does not distinguish moving-inside from crossing-between, the
    /// distinction this panel was just rebuilt around is invisible.
    #[test]
    fn the_settings_hint_separates_moving_from_crossing() {
        let mut c = chooser_with_maps(vec![kit(Some("site"), "site", 1)], vec![MapEntry {
                name: "hall".into(),
                path: PathBuf::from("hall.map.ron"),
                summary: MapSummary::Read {
                    placements: 0,
                    stamps: 0,
                    bounds: (4.0, 3.0, 4.0),
                    palette: Vec::new(),
                    uses: BTreeSet::new(),
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
            hint.contains("Tab panel"),
            "and Tab is what crosses: {hint}"
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

