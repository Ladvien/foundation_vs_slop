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
//! # Why this is a separate `App` and not a state inside the editor
//!
//! `Project` is opened before the app is built and inserted before the editor's plugins
//! (`harness.rs`), and in Bevy 0.19 a missing `Res<T>` **panics its system** (`lib.rs`, `docs/ui.md`
//! §5). Around **sixty production systems across eight files** take `Res<Project>` or
//! `ResMut<Project>`. A chooser that ran inside the editor's `App` with no project chosen yet would
//! mean gating every one of them through set machinery this crate does not have — `keys::Phase` is
//! its only `SystemSet` — where a single missed system is a first-frame panic.
//!
//! Gating is *feasible*: `resource_exists` takes `Option<Res<T>>`, so the guard itself is safe. It
//! is the **cost** that is the argument, not impossibility. So the chooser is its own `App` and
//! launches the editor as a child process: one `App` per process, `Project` always present wherever
//! the editor's plugins are, no gating and no teardown. `harness::build_headless` is untouched.
//!
//! The in-editor overlay — `Cmd+O` without a restart — needs the reload this avoids (despawn every
//! `Placement` and `StampedPiece`, rebuild `Project`, reset the undo history, `next_id`, the
//! selection, `Build` and drag state). That is deliberately a separate change, so its teardown risk
//! does not land in the same commit as this UI.
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

use std::path::{Path, PathBuf};

use emerge_core::map::Map;
use emerge_core::naming;
use emerge_core::policy::LIBRARY_FILE;

/// Where kits live, under the project root. The same directory `Project::open` resolves `--kit`
/// against, named once so the two cannot drift.
pub const EMERGE_DIR: &str = "assets/emerge";

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
    pub maps: Vec<MapEntry>,
}

/// Every kit under a project root, and every map in each.
#[derive(Clone, Debug, PartialEq)]
pub struct Catalog {
    pub kits: Vec<Kit>,
}

impl Catalog {
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

        // **Fixed order, every scan.** See the module note on Sears & Shneiderman: nothing here is
        // sorted by use, and `the_catalog_order_never_moves` is what keeps that true.
        kits.sort_by(|a, b| a.label.cmp(&b.label));
        Ok(Catalog { kits })
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
    let pieces = emerge_core::library::Library::parse(&text)
        .map_err(|e| format!("{}: {e}", library.display()))?
        .descriptors
        .len();

    let label = dir.file_name().map_or_else(
        || dir.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );

    Ok(Some(Kit {
        flag,
        label,
        dir: dir.to_path_buf(),
        pieces,
        maps: read_maps(dir)?,
    }))
}

/// Every `*.map.ron` beside a kit, alphabetical.
fn read_maps(dir: &Path) -> Result<Vec<MapEntry>, String> {
    const SUFFIX: &str = ".map.ron";
    let mut out = Vec::new();
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
    kit_dir: &Path,
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
    let path = kit_dir.join(naming::map_file_name(&name));
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

    /// A throwaway project root: `assets/emerge/` plus whatever kits a test asks for.
    struct Root(PathBuf);

    impl Root {
        fn new(name: &str) -> Root {
            let dir = std::env::temp_dir().join(format!("chooser-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let base = dir.join(EMERGE_DIR);
            std::fs::create_dir_all(&base).unwrap_or_else(|e| panic!("{}: {e}", base.display()));
            Root(dir)
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
            dir
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
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
        let kit = root.kit(Some("site"), 1);
        for m in ["zulu", "alpha", "mike"] {
            create_map(&kit, m, (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
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
            .map(|k| k.maps.iter().map(|m| m.name.as_str()).collect())
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
        let kit = root.kit(Some("site"), 1);

        let path = create_map(&kit, "Porch A", (12.0, 5.0, 9.0), (1.0, 0.0, 2.0), None)
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
            .and_then(|k| k.maps.iter().find(|m| m.name == "porch_a"))
            .unwrap_or_else(|| panic!("the new map is not in the catalog"));
        assert_eq!(
            entry.summary,
            MapSummary::Read {
                placements: 0,
                stamps: 0,
                bounds: (12.0, 5.0, 9.0)
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
            let e = create_map(&kit, raw, (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
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
        let kit = root.kit(Some("site"), 1);
        create_map(&kit, "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let e = create_map(&kit, "hall", (8.0, 3.0, 8.0), (0.0, 0.0, 0.0), None)
            .err()
            .unwrap_or_else(|| panic!("the second `hall` must be refused"));
        assert!(e.contains("already exists"), "{e}");
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
        assert!(kit.maps.is_empty());
    }

    /// **A map that will not parse is a row, not an omission.** Dropping it would present a broken
    /// project as an empty one, and the author would go looking for a map the list had quietly eaten.
    #[test]
    fn an_unreadable_map_is_listed_with_its_reason() {
        let root = Root::new("broken");
        let kit = root.kit(Some("site"), 1);
        std::fs::write(kit.join("broken.map.ron"), "(this is not a map)")
            .unwrap_or_else(|e| panic!("{e}"));

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let entry = catalog
            .kits
            .iter()
            .find(|k| k.label == "site")
            .and_then(|k| k.maps.iter().find(|m| m.name == "broken"))
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
    const ALL: [Focus; 3] = [Focus::Kits, Focus::Maps, Focus::Settings];
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
    pub const ALL: [Field; 4] = [Field::Name, Field::Bounds, Field::Origin, Field::Note];

    pub fn label(self) -> &'static str {
        match self {
            Field::Name => "NAME",
            Field::Bounds => "BOUNDS",
            Field::Origin => "ORIGIN",
            Field::Note => "NOTE",
        }
    }

    fn next(self) -> Field {
        let i = Field::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Field::ALL[(i + 1) % Field::ALL.len()]
    }

    /// The other way round, for `Shift+Tab`. Wraps, so the cycle has no dead end at either edge.
    fn prev(self) -> Field {
        let i = Field::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Field::ALL[(i + Field::ALL.len() - 1) % Field::ALL.len()]
    }
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
    /// Set when a new map is being named — there is no map row to edit yet.
    pub creating: Option<Draft>,
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
        let kit = preselect
            .and_then(|want| {
                catalog
                    .kits
                    .iter()
                    .position(|k| k.flag.as_deref() == Some(want))
            })
            .unwrap_or(0);
        Chooser {
            root,
            catalog,
            kit,
            map: 0,
            focus: Focus::Kits,
            field: Field::Name,
            editing: false,
            raw: String::new(),
            problem: None,
            creating: None,
        }
    }

    pub fn current_kit(&self) -> Option<&Kit> {
        self.catalog.kits.get(self.kit)
    }

    pub fn current_map(&self) -> Option<&MapEntry> {
        self.current_kit().and_then(|k| k.maps.get(self.map))
    }

    /// Move within whichever column has the arrows. Clamped, not wrapped: a list that wraps makes
    /// "am I at the end" unanswerable without counting.
    pub fn step(&mut self, delta: i32) {
        self.problem = None;
        match self.focus {
            Focus::Kits => {
                let n = self.catalog.kits.len();
                if n > 0 {
                    self.kit = clamp_step(self.kit, delta, n);
                    // A different kit means a different map list; landing on row 0 is the only
                    // position guaranteed to exist.
                    self.map = 0;
                }
            }
            Focus::Maps => {
                let n = self.current_kit().map_or(0, |k| k.maps.len());
                if n > 0 {
                    self.map = clamp_step(self.map, delta, n);
                }
            }
            // **The arrows walk the settings rows too**, which is the whole of the correction:
            // moving inside a panel is always the arrows, whichever panel it is.
            Focus::Settings => {
                self.field = if delta < 0 {
                    self.field.prev()
                } else {
                    self.field.next()
                };
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

    /// **What the editor would be launched with**, or why it cannot be.
    pub fn launch_args(&self) -> Result<Vec<String>, String> {
        let kit = self
            .current_kit()
            .ok_or_else(|| "no kit selected".to_owned())?;
        let map = self
            .current_map()
            .ok_or_else(|| format!("no maps in {} yet — press N to make one", kit.label))?;
        if let MapSummary::Unreadable(why) = &map.summary {
            return Err(format!("`{}` will not open: {why}", map.name));
        }
        let mut args = vec![self.root.display().to_string(), map.name.clone()];
        if let Some(flag) = &kit.flag {
            args.push("--kit".to_owned());
            args.push(flag.clone());
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

    fn kit(flag: Option<&str>, label: &str, pieces: usize, maps: Vec<MapEntry>) -> Kit {
        Kit {
            flag: flag.map(str::to_owned),
            label: label.to_owned(),
            dir: PathBuf::from(label),
            pieces,
            maps,
        }
    }

    fn chooser(preselect: Option<&str>) -> Chooser {
        let catalog = Catalog {
            kits: vec![
                kit(None, "emerge", 75, vec![ok_map("untitled_map")]),
                kit(
                    Some("site"),
                    "site",
                    45,
                    vec![ok_map("hall"), ok_map("test1")],
                ),
                kit(Some("site_v2"), "site_v2", 0, vec![]),
            ],
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
        assert_eq!(c.kit, 0, "up at the top stays");
        c.step(1);
        c.step(1);
        c.step(1);
        c.step(1);
        assert_eq!(c.kit, 2, "down past the end stays on the last");
    }

    /// Changing kit resets the map row, because row 4 of a two-map kit is not a row.
    #[test]
    fn changing_kit_lands_on_a_map_row_that_exists() {
        let mut c = chooser(Some("site"));
        c.section(1);
        c.step(1);
        assert_eq!(c.map, 1, "walked to the second map");
        c.section(-1);
        c.step(1); // -> site_v2, which has no maps at all
        assert_eq!(
            c.map, 0,
            "a new kit starts at the only row guaranteed to exist"
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
    /// nothing is the dead key `keys.rs` refuses to ship. `site_v2` has no maps and no map selected,
    /// so it has no settings to show.
    #[test]
    fn a_panel_with_no_rows_is_skipped() {
        let mut c = chooser(Some("site_v2"));
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
        c.step(-1);
        assert_eq!(c.field, Field::Note, "and it wraps backwards");
    }

    /// **The launch line, which is the whole output of this screen.** The root kit passes no
    /// `--kit` at all, because `Project::open(None)` is a real mode and a flag would change it.
    #[test]
    fn the_launch_line_carries_the_kit_only_when_there_is_one() {
        let mut c = chooser(Some("site"));
        assert_eq!(
            c.launch_args().unwrap_or_else(|e| panic!("{e}")),
            vec![".", "hall", "--kit", "site"]
        );

        c.kit = 0; // the root kit
        c.map = 0;
        assert_eq!(
            c.launch_args().unwrap_or_else(|e| panic!("{e}")),
            vec![".", "untitled_map"],
            "no --kit: that IS how the root kit is opened"
        );
    }

    /// An unmet condition is an instruction, not a report (`docs/ui.md` §1.4). An empty kit says
    /// what to press.
    #[test]
    fn an_empty_kit_says_what_to_do_about_it() {
        let c = chooser(Some("site_v2"));
        let e = c
            .launch_args()
            .err()
            .unwrap_or_else(|| panic!("nothing to open"));
        assert!(
            e.contains("press N"),
            "the refusal has to be an instruction: {e}"
        );
        assert!(e.contains("site_v2"), "and name the kit: {e}");
    }

    /// A map that would not parse is offered as a row and refused at `Enter`, with the reason — not
    /// hidden from the list, and not launched into a crash.
    #[test]
    fn an_unreadable_map_is_refused_at_the_door_with_its_reason() {
        let catalog = Catalog {
            kits: vec![kit(
                Some("site"),
                "site",
                1,
                vec![entry(
                    "broken",
                    MapSummary::Unreadable("map: bad ron".into()),
                )],
            )],
        };
        let c = Chooser::new(PathBuf::from("."), catalog, None);
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
        let mut f = Field::Name;
        let mut seen = vec![f];
        for _ in 0..4 {
            f = f.next();
            seen.push(f);
        }
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
        let mut f = Field::Note;
        let mut seen = vec![f];
        for _ in 0..4 {
            f = f.prev();
            seen.push(f);
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

        for f in Field::ALL {
            assert_eq!(
                f.next().prev(),
                f,
                "{f:?}: forward then back is where you were"
            );
            assert_eq!(f.prev().next(), f, "{f:?}: back then forward is too");
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
    pub maps_header: String,
    pub maps: Vec<Row>,
    pub settings_header: String,
    pub settings: Vec<Row>,
    pub problem: Option<String>,
    pub hint: String,
}

impl Chooser {
    /// The screen this state describes.
    pub fn screen(&self) -> Screen {
        let kits = self
            .catalog
            .kits
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let selected = self.focus == Focus::Kits && i == self.kit;
                Row {
                    left: if k.flag.is_none() {
                        format!("{}   (default)", k.label)
                    } else {
                        k.label.clone()
                    },
                    right: format!("{} pieces", k.pieces),
                    // **A blank kit reads as blank without being read.** This is the fact the screen
                    // exists to carry: on 2026-08-15 an author could not tell `site` from `site_v2`
                    // and relaunched three times. A count nobody looks at would not have helped.
                    tone: match (selected, k.pieces) {
                        (true, _) => Tone::Selected,
                        (false, 0) => Tone::Empty,
                        (false, _) => Tone::Stocked,
                    },
                }
            })
            .collect();

        let kit = self.current_kit();
        let maps_header = kit.map_or_else(|| "MAPS".to_owned(), |k| format!("MAPS IN {}", k.label));
        let maps = match kit {
            // §1.4: an unmet condition is an instruction, never a report.
            Some(k) if k.maps.is_empty() => vec![Row {
                left: format!("no maps in {} yet — press N to make one", k.label),
                right: String::new(),
                tone: Tone::Empty,
            }],
            Some(k) => k
                .maps
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let selected = self.focus == Focus::Maps && i == self.map;
                    let (right, tone) = match &m.summary {
                        MapSummary::Unreadable(why) => {
                            (format!("will not open — {why}"), Tone::Problem)
                        }
                        MapSummary::Read {
                            placements, stamps, ..
                        } => {
                            let text = match (placements, stamps) {
                                (0, 0) => "empty".to_owned(),
                                (p, 0) => format!("{p} piece(s)"),
                                (0, t) => format!("{t} tile(s)"),
                                (p, t) => format!("{p} piece(s), {t} tile(s)"),
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
                        left: m.name.clone(),
                        right,
                        tone: if selected { Tone::Selected } else { tone },
                    }
                })
                .collect(),
            None => Vec::new(),
        };

        // **The settings are shown for whatever is in hand** — the draft while one is being made,
        // and otherwise the selected map. The first version drew them only while creating, so `Tab`
        // did nothing on an existing map and three of the four settings were unreachable.
        let (settings_header, settings) = self.settings_rows();

        Screen {
            kits,
            maps_header,
            maps,
            settings_header,
            settings,
            problem: self.problem.clone(),
            hint: self.hint().to_owned(),
        }
    }

    fn settings_rows(&self) -> (String, Vec<Row>) {
        let (header, name, bounds, origin, note) = match (&self.creating, self.current_map()) {
            (Some(d), _) => (
                "NEW MAP".to_owned(),
                d.name.clone(),
                d.bounds,
                d.origin,
                d.note.clone(),
            ),
            (None, Some(m)) => match &m.summary {
                MapSummary::Read { bounds, .. } => {
                    // Origin and note are not in the summary — the row is about the file, and
                    // reading every map's prose to fill a panel nobody has opened is work for a
                    // list. Selecting one is what asks the question, so it is read here.
                    let (origin, note) = read_origin_and_note(&m.path);
                    (
                        format!("SETTINGS — {}", m.name),
                        m.name.clone(),
                        *bounds,
                        origin,
                        note,
                    )
                }
                MapSummary::Unreadable(_) => return ("SETTINGS".to_owned(), Vec::new()),
            },
            (None, None) => return ("SETTINGS".to_owned(), Vec::new()),
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
                        name
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
                right: value(Field::Note, clip(note.unwrap_or_default().as_str(), 46)),
                tone: tone_for(live(Field::Note), false),
            },
        ];
        (header, rows)
    }

    /// The verbs, and only the ones that would do something right now. `docs/ui.md` §3.5 caps
    /// immediately-issuable choices at three or four; a key listed where it is dead is worse than a
    /// key not listed, because it teaches something untrue.
    pub fn hint(&self) -> &'static str {
        match self.focus {
            _ if self.editing => "type    Enter keep    Esc cancel",
            Focus::Settings if self.creating.is_some() => {
                "up/down field    Enter edit    Tab panel    Ctrl+Enter make it    Esc cancel"
            }
            Focus::Settings => "up/down field    Enter edit    Tab panel    Esc quit",
            Focus::Kits if self.current_kit().is_some_and(|k| k.maps.is_empty()) => {
                "up/down kit    Tab panel    N new map    Esc quit"
            }
            Focus::Kits => "up/down kit    Tab panel    Enter open    N new map    Esc quit",
            Focus::Maps => "up/down map    Tab panel    Enter open    N new map    Esc quit",
        }
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
use std::sync::{Arc, Mutex};

/// **Where the chosen launch line comes back out.**
///
/// `App::run()` consumes the world, so the choice cannot be read off a resource afterwards. The
/// chooser writes here and asks the app to exit; `main.rs` reads it once `run` returns.
pub type Choice = Arc<Mutex<Option<Vec<String>>>>;

#[derive(Resource, Clone)]
struct ChoiceOut(Choice);

#[derive(Component)]
struct KitList;
#[derive(Component)]
struct MapList;
#[derive(Component)]
struct MapsHeader;
#[derive(Component)]
struct SettingsList;
#[derive(Component)]
struct SettingsHeader;
#[derive(Component)]
struct ProblemLine;
#[derive(Component)]
struct HintLine;

/// The chooser's screen. **Not** part of `harness::add_editor_plugins` — it is the other half of the
/// binary, and adding it there would put a second `Camera2d` in every editor `App`.
pub struct ChooserPlugin {
    pub chooser: Chooser,
    pub out: Choice,
}

impl Plugin for ChooserPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Chooser {
            root: self.chooser.root.clone(),
            catalog: self.chooser.catalog.clone(),
            kit: self.chooser.kit,
            map: self.chooser.map,
            focus: self.chooser.focus,
            field: self.chooser.field,
            editing: self.chooser.editing,
            raw: self.chooser.raw.clone(),
            problem: self.chooser.problem.clone(),
            creating: self.chooser.creating.clone(),
        })
        .insert_resource(ChoiceOut(self.out.clone()))
        .add_systems(Startup, spawn_screen)
        // **Text before chords, as `keys::Phase` orders them in the editor**: a field with the
        // keyboard consumes a keystroke before anything reads it as a verb, or typing `n` into a
        // name starts a second new map.
        .add_systems(
            Update,
            (type_into_field, drive_chooser, paint_chooser).chain(),
        );
    }
}

/// One column's width. **Both lists get the same one**, and the settings panel below spans exactly
/// two of them plus the gap — so the three panels share a grid instead of each sizing itself to its
/// longest row, which is what made the first version read as unrelated blobs.
const COL: f32 = 330.0;

/// **The interface scale, and both halves of the binary read it from here.**
///
/// `UiScale` multiplies every `Val::Px` and every font size, so a window sized in raw pixels is a
/// window that does not fit its own content — which is exactly what happened: a 672 px settings
/// panel at 1.2 is 806, inside a 740 px window, and the values ran off the right edge.
pub const UI_SCALE: f32 = 1.2;

/// **How big the window has to be to hold this screen**, derived rather than guessed.
///
/// Two columns and the gap, plus the root's padding on both sides, times the scale. Returned so
/// `main.rs` cannot fall out of step with `COL` — the previous version hard-coded a size and was
/// wrong the moment the columns were given a fixed width.
pub fn window_size() -> (f32, f32) {
    let content = COL * 2.0 + crate::chrome::PAD;
    let width = (content + crate::chrome::PAD * 3.0) * UI_SCALE;
    // Title, the list row, the settings panel, the hint, and the gaps — measured off a capture
    // rather than computed from font metrics, which would be a second layout engine. The slack is
    // deliberate and small: a kit with several maps grows the list row, and a window that has to be
    // resized to see the last map is worse than one with a little air at the bottom.
    let height = 360.0 * UI_SCALE;
    (width, height)
}

fn panel(width: f32) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(crate::chrome::PAD)),
            row_gap: Val::Px(crate::chrome::GAP_ROW),
            width: Val::Px(width),
            ..default()
        },
        BackgroundColor(crate::chrome::PANEL_BG),
    )
}

fn header(text: &str) -> impl Bundle {
    (
        Text::new(text.to_owned()),
        TextFont::from_font_size(11.0),
        TextColor(crate::chrome::LABEL),
    )
}

fn spawn_screen(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(crate::chrome::PAD * 1.5)),
                row_gap: Val::Px(crate::chrome::GAP_ROW * 2.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.035, 0.033, 0.030)),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("emerge-mapper"),
                TextFont::from_font_size(13.0),
                TextColor(crate::chrome::LABEL),
            ));
            // **The two lists side by side.** Stacked, they left the right half of the window empty
            // and read as two unrelated blobs; a kit and its maps are one question.
            root.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(crate::chrome::PAD),
                ..default()
            })
            .with_children(|row| {
                row.spawn(panel(COL)).with_children(|p| {
                    p.spawn(header("KITS"));
                    p.spawn((Node::default(), KitList));
                });
                row.spawn(panel(COL)).with_children(|p| {
                    p.spawn((header("MAPS"), MapsHeader));
                    p.spawn((Node::default(), MapList));
                });
            });
            root.spawn(panel(COL * 2.0 + crate::chrome::PAD))
                .with_children(|p| {
                    p.spawn((header("SETTINGS"), SettingsHeader));
                    p.spawn((Node::default(), SettingsList));
                });
            root.spawn((
                Text::new(String::new()),
                TextFont::from_font_size(12.0),
                TextColor(crate::chrome::DANGER),
                ProblemLine,
            ));
            root.spawn((
                Text::new(String::new()),
                TextFont::from_font_size(11.0),
                TextColor(crate::chrome::LABEL),
                HintLine,
            ));
        });
}

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
fn fill(commands: &mut Commands, at: Entity, rows: &[Row]) {
    commands.entity(at).despawn_related::<Children>();
    commands.entity(at).insert(Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(crate::chrome::GAP_ROW * 0.6),
        ..default()
    });
    for r in rows {
        let c = colour(r.tone);
        let mark = if r.tone == Tone::Selected { ">" } else { " " };
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
                line.spawn((
                    Text::new(left.clone()),
                    TextFont::from_font_size(13.0),
                    TextColor(c),
                ));
                if !right.is_empty() {
                    line.spawn((
                        Text::new(right.clone()),
                        TextFont::from_font_size(13.0),
                        TextColor(c),
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
    )>,
    mut texts: Query<(
        &mut Text,
        Option<&MapsHeader>,
        Option<&SettingsHeader>,
        Option<&ProblemLine>,
        Option<&HintLine>,
    )>,
) {
    if !chooser.is_changed() {
        return;
    }
    let s = chooser.screen();
    for (e, kit, map, set) in &lists {
        if kit.is_some() {
            fill(&mut commands, e, &s.kits);
        } else if map.is_some() {
            fill(&mut commands, e, &s.maps);
        } else if set.is_some() {
            fill(&mut commands, e, &s.settings);
        }
    }
    for (mut text, maps, settings, problem, hint) in &mut texts {
        if maps.is_some() {
            **text = s.maps_header.clone();
        } else if settings.is_some() {
            **text = s.settings_header.clone();
        } else if problem.is_some() {
            **text = s.problem.clone().unwrap_or_default();
        } else if hint.is_some() {
            **text = s.hint.clone();
        }
    }
}

/// **The field takes the keyboard first.** Mirrors `build.rs`'s name prompt, including the drain:
/// while no field is open the stream is cleared, so the keystroke that *opens* one cannot become its
/// first character (the `xseam` bug this crate already paid for once).
fn type_into_field(
    mut events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut chooser: ResMut<Chooser>,
) {
    if !chooser.editing {
        events.clear();
        return;
    }
    let field = chooser.field;
    let _ = &keyboard;
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            // **`Enter` keeps it and stops typing** — it does not jump to the next field. Moving
            // between fields is the arrows now, and a commit that also moved would be the same key
            // doing two jobs, which is what this screen's `Tab` was just corrected for.
            Key::Enter => {
                if commit_field(&mut chooser, field) {
                    chooser.editing = false;
                }
                return;
            }
            Key::Escape => {
                chooser.raw.clear();
                chooser.problem = None;
                chooser.editing = false;
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

/// Parse and store one field, or refuse it by name. **Nothing is substituted** — a value that will
/// not parse leaves the old one alone and says why.
///
/// Answers **whether it committed**, and deliberately does not decide where the keyboard goes next:
/// `Enter` and `Tab` advance, `Shift+Tab` goes back, and a refusal keeps you on the field whichever
/// key you pressed. Choosing the destination in here made that one behaviour with three callers.
fn commit_field(chooser: &mut Chooser, field: Field) -> bool {
    let raw = chooser.raw.trim().to_owned();
    // Editing an existing map writes that file; making one fills in a draft first.
    let existing = chooser.creating.is_none();
    let mut draft = match (&chooser.creating, chooser.current_map()) {
        (Some(d), _) => d.clone(),
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
        chooser.creating = Some(draft);
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
        return Err(format!("`{}` already exists in this kit", map.name));
    }
    let out = ron::ser::to_string_pretty(&map, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("map: serialize: {e}"))?;
    emerge_core::ron_surgery::save_atomic(&path, &out)?;
    // Follow a rename, exactly as `Project::save` does — the file a map is in is the file its name
    // says it is.
    if path != old_path {
        let _ = std::fs::remove_file(&old_path);
    }
    let name = map.name.clone();
    rescan_keeping_place(chooser, Some(&name));
    Ok(())
}

/// Rescan and land on a named map, so the list is always a description of disk.
fn rescan_keeping_place(chooser: &mut Chooser, want: Option<&str>) {
    let label = chooser.current_kit().map(|k| k.label.clone());
    match Catalog::scan(&chooser.root.clone()) {
        Err(e) => chooser.problem = Some(e),
        Ok(catalog) => {
            chooser.catalog = catalog;
            chooser.kit = label
                .and_then(|l| chooser.catalog.kits.iter().position(|k| k.label == l))
                .unwrap_or(0);
            chooser.map = want
                .and_then(|w| {
                    chooser
                        .current_kit()
                        .and_then(|k| k.maps.iter().position(|m| m.name == w))
                })
                .unwrap_or(0);
        }
    }
}

/// Three whitespace- or comma-separated numbers, or nothing. Refuses rather than filling in a
/// missing axis, because a bounds triple with a guessed Y is a map of a height nobody chose.
fn parse_triple(raw: &str) -> Option<(f32, f32, f32)> {
    let parts: Vec<f32> = raw
        .split(|c: char| c.is_whitespace() || c == ',' || c == 'x')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f32>().ok())
        .collect();
    match parts[..] {
        [x, y, z] => Some((x, y, z)),
        _ => None,
    }
}

fn drive_chooser(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut chooser: ResMut<Chooser>,
    out: Res<ChoiceOut>,
    mut exit: MessageWriter<AppExit>,
) {
    // Typing owns the keyboard; `type_into_field` ran first and has already consumed it.
    if chooser.editing {
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
    if keyboard.just_pressed(KeyCode::KeyN) {
        chooser.creating = Some(Draft::default());
        chooser.raw.clear();
        chooser.problem = None;
        chooser.field = Field::Name;
        chooser.focus = Focus::Settings;
        // Straight into the name, because a new map has nothing else worth looking at yet and the
        // name is the one thing it cannot be saved without.
        chooser.editing = true;
    }
    if keyboard.just_pressed(KeyCode::Escape) {
        if chooser.creating.is_some() {
            chooser.creating = None;
            chooser.problem = None;
        } else {
            exit.write(AppExit::Success);
        }
    }
    if keyboard.just_pressed(KeyCode::Enter) {
        // **`Ctrl+Enter` makes the map**, because plain `Enter` in this panel now means "edit this
        // row". Two verbs on one panel need two keys; overloading `Enter` by whether the name
        // happens to be filled in is the kind of state-decides-the-verb rule this screen just lost.
        let commit_new = keyboard.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ]);
        if let Some(draft) = chooser.creating.clone()
            && commit_new
        {
            let Some(dir) = chooser.current_kit().map(|k| k.dir.clone()) else {
                chooser.problem = Some("no kit selected".to_owned());
                return;
            };
            match create_map(
                &dir,
                &draft.name,
                draft.bounds,
                draft.origin,
                draft.note.clone(),
            ) {
                Err(e) => chooser.problem = Some(e),
                Ok(_) => {
                    let name = draft.name.clone();
                    chooser.creating = None;
                    chooser.focus = Focus::Maps;
                    rescan_keeping_place(&mut chooser, Some(&name));
                }
            }
            return;
        }
        // In the settings, `Enter` opens the highlighted row for typing.
        if chooser.focus == Focus::Settings {
            chooser.raw.clear();
            chooser.problem = None;
            chooser.editing = true;
            return;
        }
        match chooser.launch_args() {
            Err(e) => chooser.problem = Some(e),
            Ok(args) => {
                if let Ok(mut slot) = out.0.lock() {
                    *slot = Some(args);
                }
                exit.write(AppExit::Success);
            }
        }
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;

    fn chooser_with(kits: Vec<Kit>) -> Chooser {
        Chooser::new(PathBuf::from("."), Catalog { kits }, None)
    }

    fn kit(flag: Option<&str>, label: &str, pieces: usize, maps: Vec<MapEntry>) -> Kit {
        Kit {
            flag: flag.map(str::to_owned),
            label: label.to_owned(),
            dir: PathBuf::from(label),
            pieces,
            maps,
        }
    }

    /// **The piece count is on screen.** It is the fact the whole chooser exists to show — the one
    /// that was unavailable on 2026-08-15 when an author could not tell which kit was loaded and
    /// relaunched three times against the wrong one.
    #[test]
    fn the_screen_says_how_many_pieces_each_kit_holds() {
        let c = chooser_with(vec![
            kit(Some("site"), "site", 45, vec![]),
            kit(Some("site_v2"), "site_v2", 0, vec![]),
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
        let c = chooser_with(vec![kit(None, "emerge", 75, vec![])]);
        assert!(render(&c).contains("(default)"), "{}", render(&c));
    }

    /// **An unmet condition is an instruction** (`docs/ui.md` §1.4). An empty kit does not report
    /// "no maps found"; it says which key makes one.
    #[test]
    fn an_empty_kit_reads_as_an_instruction_not_a_report() {
        let c = chooser_with(vec![kit(Some("site_v2"), "site_v2", 0, vec![])]);
        let screen = render(&c);
        assert!(screen.contains("press N to make one"), "{screen}");
        assert!(
            !screen.contains("not found") && !screen.contains("no maps found"),
            "a report where an instruction belongs:\n{screen}"
        );
    }

    /// A map that will not parse says so on its own row, rather than being quietly absent — the
    /// author would otherwise go looking for a map the list had eaten.
    #[test]
    fn a_broken_map_is_visible_and_says_it_will_not_open() {
        let c = chooser_with(vec![kit(
            Some("site"),
            "site",
            1,
            vec![MapEntry {
                name: "broken".into(),
                path: PathBuf::from("broken.map.ron"),
                summary: MapSummary::Unreadable("map: bad ron".into()),
            }],
        )]);
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
        let stocked = chooser_with(vec![kit(
            Some("site"),
            "site",
            1,
            vec![MapEntry {
                name: "hall".into(),
                path: PathBuf::from("hall.map.ron"),
                summary: MapSummary::Read {
                    placements: 0,
                    stamps: 0,
                    bounds: (4.0, 3.0, 4.0),
                },
            }],
        )]);
        let screen = render(&stocked);
        for verb in ["Enter open", "N new map", "Esc quit"] {
            assert!(
                screen.contains(verb),
                "`{verb}` is not on screen:\n{screen}"
            );
        }

        let empty = chooser_with(vec![kit(Some("site_v2"), "site_v2", 0, vec![])]);
        let screen = render(&empty);
        assert!(
            !screen.contains("Enter open"),
            "there is nothing to open, so offering the key teaches a lie:\n{screen}"
        );
        assert!(
            screen.contains("N new map"),
            "and the live verb is still there:\n{screen}"
        );
    }

    /// **The settings hint says which key does which job**, because neither has a visual affordance
    /// — ExposeHK's own caveat about techniques with "no visual representation to aid their
    /// discovery". If the line does not distinguish moving-inside from crossing-between, the
    /// distinction this panel was just rebuilt around is invisible.
    #[test]
    fn the_settings_hint_separates_moving_from_crossing() {
        let mut c = chooser_with(vec![kit(
            Some("site"),
            "site",
            1,
            vec![MapEntry {
                name: "hall".into(),
                path: PathBuf::from("hall.map.ron"),
                summary: MapSummary::Read {
                    placements: 0,
                    stamps: 0,
                    bounds: (4.0, 3.0, 4.0),
                },
            }],
        )]);
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
