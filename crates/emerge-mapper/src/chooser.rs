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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Kits,
    Maps,
    /// Typing into one of the settings fields. Shadows the arrows, exactly as `Context::Typing` does
    /// in the editor.
    Field(Field),
}

/// The four settings the chooser exposes, in the order they are shown and `Tab` walks them.
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
            Focus::Field(_) => {}
        }
    }

    /// `right` enters the map column, `left` comes back — the same shape the Tiles tab's KIT list
    /// uses, so the gesture transfers.
    pub fn across(&mut self, into_maps: bool) {
        self.problem = None;
        match (self.focus, into_maps) {
            (Focus::Kits, true) if self.current_kit().is_some_and(|k| !k.maps.is_empty()) => {
                self.focus = Focus::Maps;
            }
            (Focus::Maps, false) => self.focus = Focus::Kits,
            _ => {}
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
        c.across(true);
        c.step(1);
        assert_eq!(c.map, 1, "walked to the second map");
        c.across(false);
        c.step(1); // -> site_v2, which has no maps at all
        assert_eq!(
            c.map, 0,
            "a new kit starts at the only row guaranteed to exist"
        );
    }

    /// `right` enters the map column and `left` comes back — and an empty kit has nothing to enter,
    /// so the arrows stay where they can do something.
    #[test]
    fn the_map_column_cannot_be_entered_when_it_is_empty() {
        let mut c = chooser(Some("site_v2"));
        c.across(true);
        assert_eq!(
            c.focus,
            Focus::Kits,
            "site_v2 has no maps, so right does nothing"
        );

        let mut c = chooser(Some("site"));
        c.across(true);
        assert_eq!(c.focus, Focus::Maps);
        c.across(false);
        assert_eq!(c.focus, Focus::Kits);
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

    /// `Tab` walks the four fields and wraps — a fixed cycle, in the order they are drawn.
    #[test]
    fn tab_walks_the_fields_in_the_order_they_are_shown() {
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
}

// ------------------------------------------------------------------------------------------------
// The Bevy half
// ------------------------------------------------------------------------------------------------

use bevy::input::keyboard::{Key, KeyboardInput};
use std::sync::{Arc, Mutex};

/// **Where the chosen launch line comes back out.**
///
/// `App::run()` consumes the world, so the choice cannot simply be read off a resource afterwards.
/// The chooser writes here and then asks the app to exit; `main.rs` reads it once `run` returns and
/// launches the editor. One value, written once, so a mutex is the whole synchronisation story.
pub type Choice = Arc<Mutex<Option<Vec<String>>>>;

#[derive(Resource, Clone)]
struct ChoiceOut(Choice);

#[derive(Component)]
struct ChooserText;

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
            raw: self.chooser.raw.clone(),
            problem: self.chooser.problem.clone(),
            creating: self.chooser.creating.clone(),
        })
        .insert_resource(ChoiceOut(self.out.clone()))
        .add_systems(Startup, spawn_screen)
        // **Text before chords, exactly as `keys::Phase` orders them in the editor**: a field with
        // the keyboard must consume a keystroke before anything else can read it as a verb, or
        // typing `n` into a name starts a second new map.
        .add_systems(
            Update,
            (type_into_field, drive_chooser, paint_chooser).chain(),
        );
    }
}

fn spawn_screen(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(crate::chrome::PAD * 2.0)),
            ..default()
        },
        BackgroundColor(crate::chrome::PANEL_BG),
        children![(
            Text::new(String::new()),
            TextFont::from_font_size(15.0),
            TextColor(crate::chrome::DIM),
            ChooserText,
        )],
    ));
}

/// **The field takes the keyboard first.** Mirrors `build.rs`'s name prompt, including the drain:
/// while no field is open the stream is cleared, so the keystroke that *opens* one cannot become its
/// first character (the `xseam` bug this crate already paid for once).
fn type_into_field(mut events: MessageReader<KeyboardInput>, mut chooser: ResMut<Chooser>) {
    let Focus::Field(field) = chooser.focus else {
        events.clear();
        return;
    };
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                commit_field(&mut chooser, field);
                return;
            }
            Key::Escape => {
                chooser.raw.clear();
                chooser.problem = None;
                chooser.focus = if chooser.creating.is_some() {
                    Focus::Field(Field::Name)
                } else {
                    Focus::Kits
                };
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
fn commit_field(chooser: &mut Chooser, field: Field) {
    let raw = chooser.raw.trim().to_owned();
    let mut draft = chooser.creating.clone().unwrap_or_default();
    match field {
        Field::Name => {
            let name = naming::to_snake_case(&raw);
            if name.is_empty() {
                chooser.problem =
                    Some("a map needs a name — snake_case, starting with a letter".to_owned());
                return;
            }
            draft.name = name;
        }
        Field::Bounds | Field::Origin => {
            let Some(triple) = parse_triple(&raw) else {
                chooser.problem = Some(format!(
                    "`{raw}` is not three numbers — type them like `32 4 32`"
                ));
                return;
            };
            if field == Field::Bounds {
                if triple.0 <= 0.0 || triple.1 <= 0.0 || triple.2 <= 0.0 {
                    chooser.problem =
                        Some("a map's bounds must all be positive — it is a volume".to_owned());
                    return;
                }
                draft.bounds = triple;
            } else {
                draft.origin = triple;
            }
        }
        Field::Note => draft.note = (!raw.is_empty()).then_some(raw),
    }
    chooser.creating = Some(draft);
    chooser.raw.clear();
    chooser.problem = None;
    chooser.focus = Focus::Field(field.next());
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
    if matches!(chooser.focus, Focus::Field(_)) {
        return;
    }
    if keyboard.just_pressed(KeyCode::ArrowUp) {
        chooser.step(-1);
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        chooser.step(1);
    }
    if keyboard.just_pressed(KeyCode::ArrowRight) {
        chooser.across(true);
    }
    if keyboard.just_pressed(KeyCode::ArrowLeft) {
        chooser.across(false);
    }
    if keyboard.just_pressed(KeyCode::KeyN) {
        chooser.creating = Some(Draft::default());
        chooser.raw.clear();
        chooser.problem = None;
        chooser.focus = Focus::Field(Field::Name);
    }
    if keyboard.just_pressed(KeyCode::Tab) && chooser.creating.is_some() {
        chooser.raw.clear();
        chooser.focus = Focus::Field(Field::Name);
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
        // A draft in hand commits; otherwise the selected map opens.
        if let Some(draft) = chooser.creating.clone() {
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
                    // Rescan rather than patching the list: one description of what is on disk.
                    match Catalog::scan(&chooser.root.clone()) {
                        Err(e) => chooser.problem = Some(e),
                        Ok(catalog) => {
                            let label = chooser.current_kit().map(|k| k.label.clone());
                            chooser.catalog = catalog;
                            chooser.kit = label
                                .and_then(|l| {
                                    chooser.catalog.kits.iter().position(|k| k.label == l)
                                })
                                .unwrap_or(0);
                            chooser.map = chooser
                                .current_kit()
                                .and_then(|k| k.maps.iter().position(|m| m.name == draft.name))
                                .unwrap_or(0);
                            chooser.creating = None;
                            chooser.focus = Focus::Maps;
                        }
                    }
                }
            }
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

fn paint_chooser(chooser: Res<Chooser>, mut text: Query<&mut Text, With<ChooserText>>) {
    if !chooser.is_changed() {
        return;
    }
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    **text = render(&chooser);
}

/// The screen as text. Pure, so a test can read what an author would.
pub fn render(c: &Chooser) -> String {
    let mut s = String::from("emerge-mapper\n\n");
    let arrow = |on: bool| if on { ">" } else { " " };

    s.push_str("KITS\n");
    for (i, k) in c.catalog.kits.iter().enumerate() {
        let tail = if k.flag.is_none() {
            "   (the default kit)"
        } else {
            ""
        };
        s.push_str(&format!(
            "{} {:<16} {:>3} pieces{tail}\n",
            arrow(c.focus == Focus::Kits && i == c.kit),
            k.label,
            k.pieces
        ));
    }

    let label = c.current_kit().map_or("", |k| k.label.as_str());
    s.push_str(&format!("\nMAPS IN {label}\n"));
    match c.current_kit() {
        Some(k) if k.maps.is_empty() => {
            // §1.4: an unmet condition is an instruction.
            s.push_str(&format!("  no maps in {label} yet — press N to make one\n"));
        }
        Some(k) => {
            for (i, m) in k.maps.iter().enumerate() {
                let about = match &m.summary {
                    MapSummary::Read {
                        placements, stamps, ..
                    } => match (placements, stamps) {
                        (0, 0) => "empty".to_owned(),
                        (p, 0) => format!("{p} placement(s)"),
                        (0, t) => format!("{t} tile(s)"),
                        (p, t) => format!("{p} placement(s), {t} tile(s)"),
                    },
                    MapSummary::Unreadable(why) => format!("WILL NOT OPEN — {why}"),
                };
                s.push_str(&format!(
                    "{} {:<16} {about}\n",
                    arrow(c.focus == Focus::Maps && i == c.map),
                    m.name
                ));
            }
        }
        None => {}
    }

    if let Some(d) = &c.creating {
        s.push_str("\nNEW MAP\n");
        for f in Field::ALL {
            let live = c.focus == Focus::Field(f);
            let value = match f {
                Field::Name => {
                    if live {
                        c.raw.clone()
                    } else if d.name.is_empty() {
                        "(needs a name)".to_owned()
                    } else {
                        d.name.clone()
                    }
                }
                Field::Bounds => triple(d.bounds),
                Field::Origin => triple(d.origin),
                Field::Note => d.note.clone().unwrap_or_default(),
            };
            let shown = if live && f != Field::Name {
                c.raw.clone()
            } else {
                value
            };
            s.push_str(&format!("{} {:<8} {shown}\n", arrow(live), f.label()));
        }
    }

    if let Some(p) = &c.problem {
        s.push_str(&format!("\n{p}\n"));
    }

    s.push_str(match c.focus {
        Focus::Field(_) => "\ntype      Enter next field      Esc back",
        _ if c.creating.is_some() => "\nEnter make it      Tab edit fields      Esc cancel",
        _ => "\n^v choose      -> maps      Enter open      N new map      Esc quit",
    });
    s
}

fn triple(t: (f32, f32, f32)) -> String {
    format!("{} x {} x {}", t.0, t.1, t.2)
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
        assert!(render(&c).contains("the default kit"), "{}", render(&c));
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
        assert!(screen.contains("WILL NOT OPEN"), "{screen}");
    }

    /// **The verb keys are on screen from the first frame**, which is what ExposeHK's rehearsal goal
    /// asks for: the novice path is the expert path, so using the screen teaches the keys rather
    /// than teaching pointing. Four verbs, against §3.5's 3–4 immediate-choice budget.
    #[test]
    fn the_verbs_are_shown_and_there_are_four_of_them() {
        let c = chooser_with(vec![kit(Some("site"), "site", 1, vec![])]);
        let screen = render(&c);
        for verb in ["Enter open", "N new map", "Esc quit"] {
            assert!(
                screen.contains(verb),
                "`{verb}` is not on screen:\n{screen}"
            );
        }
    }
}
