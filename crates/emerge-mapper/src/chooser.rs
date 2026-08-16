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

/// **The exit code the editor uses to say "take me back to the menu".**
///
/// The editor is a child process of the chooser, so going back is a process boundary: the editor
/// exits with this and `main.rs`'s loop shows the menu again instead of quitting. Any other code is
/// an ordinary exit and ends the run — which is what closing the window does.
///
/// 64 rather than 1, so it cannot be confused with a crash or with a refusal from `Project::open`.
pub const BACK_TO_MENU: u8 = 64;

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

    // `face_bands: 1` — a subunit is `grid::SNAP` itself, the half-metre grid these kits are
    // authored on. Raising it refines every piece by the same factor, which is what keeps two faces
    // comparable, so it is a decision about the whole kit and belongs in this file from the start.
    let policy = format!(
        "(\n    version: 1,\n    note: Some(\"The policy layer for `{name}`. No patches yet: \
         `Project::open` refuses a rule that matches nothing, so one cannot be added before the \
         pieces it names exist.\"),\n    face_bands: 1,\n)\n"
    );
    emerge_core::ron_surgery::save_atomic(&dir.join(emerge_core::policy::POLICY_FILE), &policy)?;
    Ok(dir)
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

    /// **Escape unwinds one layer at a time and never quits on the first press.**
    ///
    /// Reported at the keyboard: typing into a field and pressing Escape *closed the whole
    /// program*. Two causes, both fixed and both pinned here — the field handler now marks the key
    /// as taken so the chord handler cannot read the same press again, and quitting is a question
    /// rather than an act.
    #[test]
    fn escape_backs_out_one_layer_at_a_time() {
        let root = Root::new("escape-stack");
        let kit = root.kit(Some("site"), 1);
        create_map(&kit, "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
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
        assert_eq!(c.focus, Focus::Settings, "you stay in the panel you were in");
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
        let kit = root.kit(Some("site"), 1);
        create_map(&kit, "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
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
        assert!(hint.contains('Y') && hint.contains("Esc"), "same two answers: {hint}");
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
        let kit = root.kit(Some("site"), 2);
        create_map(&kit, "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));

        let s = c.screen();
        assert_eq!(s.kits.first().map(|r| r.left.as_str()), Some("+ new kit"));
        assert_eq!(s.kits.first().map(|r| r.right.as_str()), Some("N"), "the key rides the row");
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

    /// `N` makes a new one of whatever the column lists — a kit on the kit list, a map on the map
    /// list. One rule, which is what makes it guessable.
    #[test]
    fn n_makes_a_new_one_of_whatever_this_column_lists() {
        let root = Root::new("n-per-panel");
        let kit = root.kit(Some("site"), 1);
        create_map(&kit, "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));

        assert_eq!(c.focus, Focus::Kits);
        c.start_new();
        assert!(matches!(c.creating, Some(New::Kit(_))), "on the kit list, a kit");
        assert!(c.editing, "and straight into the name — neither can be made without one");
        assert!(
            render(&c).contains("NEW KIT"),
            "the settings column says which:\n{}",
            render(&c)
        );

        c.creating = None;
        c.focus = Focus::Maps;
        c.start_new();
        assert!(matches!(c.creating, Some(New::Map(_))), "on the map list, a map");
        assert!(render(&c).contains("NEW MAP IN site"), "and names its kit:\n{}", render(&c));
    }

    /// **A new kit is one the editor will actually open**, which is the only thing that makes it a
    /// kit: `Catalog::scan` and `Project::open` both test for `library.ron`, so writing the
    /// directory without it would produce something the chooser lists and the editor then refuses.
    #[test]
    fn a_new_kit_is_one_the_scanner_accepts() {
        let root = Root::new("new-kit");
        root.kit(Some("site"), 3);

        let dir = create_kit(&root.0, "Site V3").unwrap_or_else(|e| panic!("{e}"));
        assert!(dir.ends_with("site_v3"), "forced to snake_case: {}", dir.display());
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
        assert!(made.maps.is_empty(), "and with no maps");
        assert_eq!(made.flag.as_deref(), Some("site_v3"), "reachable as --kit site_v3");
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
            catalog.kits.iter().find(|k| k.label == "site").map(|k| k.pieces),
            Some(1),
            "refusing must not have emptied the existing kit"
        );
    }

    /// **Asking to delete deletes nothing.** The whole point of the confirmation: the file is still
    /// there after the question is raised, and only the second keystroke removes it.
    #[test]
    fn asking_to_delete_does_not_delete() {
        let root = Root::new("ask-delete");
        let kit = root.kit(Some("site"), 1);
        let path = create_map(&kit, "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
            .unwrap_or_else(|e| panic!("{e}"));

        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        c.section(1); // into the map panel
        c.ask_delete().unwrap_or_else(|e| panic!("{e}"));

        assert!(matches!(c.ask, Some(Ask::Delete(_))), "the question is up");
        assert!(path.is_file(), "and the file is UNTOUCHED until it is answered");
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
        let kit = root.kit(Some("site"), 1);
        let path = create_map(&kit, "hall", (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
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
            create_map(&kit, m, (4.0, 3.0, 4.0), (0.0, 0.0, 0.0), None)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        c.section(1);
        c.ask_delete().unwrap_or_else(|e| panic!("{e}")); // asks about `alpha`

        c.map = 1; // the selection moves to `beta`
        let gone = c.confirm_delete().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(gone, "alpha", "the question named alpha, so alpha is what goes");
        assert!(kit.join("beta.map.ron").is_file(), "beta was never in question");
        assert!(!kit.join("alpha.map.ron").exists());
    }

    /// Pressing Delete with the arrows on the kit list is a refusal that says what to do, not a
    /// silent no-op (`docs/ui.md` §1.4).
    #[test]
    fn delete_outside_the_map_panel_says_what_to_do() {
        let root = Root::new("wrong-panel");
        root.kit(Some("site"), 1);
        let catalog = Catalog::scan(&root.0).unwrap_or_else(|e| panic!("{e}"));
        let mut c = Chooser::new(root.0.clone(), catalog, Some("site"));
        assert_eq!(c.focus, Focus::Kits);
        let e = c
            .ask_delete()
            .err()
            .unwrap_or_else(|| panic!("must refuse from the kit panel"));
        assert!(e.contains("Tab to the map panel"), "{e}");
        assert!(c.ask.is_none());
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
    pub map: String,
    pub path: PathBuf,
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
        let map = catalog
            .kits
            .get(kit.saturating_sub(1))
            .map_or(0, |k| Chooser::first_real(k.maps.len()));
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

    pub fn current_map(&self) -> Option<&MapEntry> {
        let kit = self.current_kit()?;
        kit.maps.get(self.map.checked_sub(1)?)
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

    /// Move within whichever column has the arrows. Clamped, not wrapped: a list that wraps makes
    /// "am I at the end" unanswerable without counting.
    pub fn step(&mut self, delta: i32) {
        self.problem = None;
        match self.focus {
            Focus::Kits => {
                // `+ 1` for the `+ new kit` row, which is always there — even in a project whose
                // every kit was deleted, where it is the only thing left to press.
                self.kit = clamp_step(self.kit, delta, self.catalog.kits.len() + 1);
                // A different kit means a different map list, so land on its first real row.
                self.map = Chooser::first_real(self.current_kit().map_or(0, |k| k.maps.len()));
            }
            Focus::Maps => {
                let n = self.current_kit().map_or(0, |k| k.maps.len());
                self.map = clamp_step(self.map, delta, n + 1);
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
        if self.focus != Focus::Maps {
            return Err("select a map first — Tab to the map panel, then Delete".to_owned());
        }
        let m = self
            .current_map()
            .ok_or_else(|| "there is no map here to delete".to_owned())?;
        self.ask = Some(Ask::Delete(Pending {
            map: m.name.clone(),
            path: m.path.clone(),
        }));
        Ok(())
    }

    /// **Agree to it.** Removes the file the question named, then rescans so the list is a
    /// description of disk rather than of the edit.
    pub fn confirm_delete(&mut self) -> Result<String, String> {
        let Some(Ask::Delete(pending)) = self.ask.take() else {
            return Err("no deletion was asked about".to_owned());
        };
        std::fs::remove_file(&pending.path)
            .map_err(|e| format!("could not delete `{}`: {e}", pending.map))?;
        rescan_keeping_place(self, None);
        if self.current_map().is_none() {
            self.focus = Focus::Kits;
        }
        Ok(pending.map)
    }

    /// **What the editor would be launched with**, or why it cannot be.
    pub fn launch_args(&self) -> Result<Vec<String>, String> {
        let kit = self
            .current_kit()
            .ok_or_else(|| "no kit selected".to_owned())?;
        if self.on_new_row() {
            return Err("that row makes a new map — press Enter on it".to_owned());
        }
        let map = self.current_map().ok_or_else(|| {
            format!("no maps in {} yet — press Enter on `+ new map`", kit.label)
        })?;
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
        assert_eq!(c.kit, 0, "up at the top stays — and the top is the `+ new kit` row");
        for _ in 0..6 {
            c.step(1);
        }
        assert_eq!(
            c.kit, 3,
            "three kits sit at rows 1..=3, and walking past the end stays on the last"
        );
    }

    /// Changing kit resets the map row, because row 4 of a two-map kit is not a row.
    #[test]
    fn changing_kit_lands_on_a_map_row_that_exists() {
        let mut c = chooser(Some("site"));
        c.section(1);
        c.step(1);
        assert_eq!(c.map, 2, "row 1 is the first map; row 0 makes a new one");
        c.section(-1);
        c.step(1); // -> site_v2, which has no maps at all
        assert_eq!(
            c.map, 0,
            "an empty kit leaves only the `+ new map` row to be on"
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

        c.kit = 1; // the root kit — row 0 is `+ new kit`
        c.map = 1;
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
            e.contains("+ new map"),
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
        kits.extend(self
            .catalog
            .kits
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let selected = self.focus == Focus::Kits && i + 1 == self.kit;
                Row {
                    left: if k.flag.is_none() {
                        format!("{} (default)", k.label)
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
            }));

        let kit = self.current_kit();
        let maps_header = kit.map_or_else(
            || "MAPS".to_owned(),
            |k| format!("MAPS IN {}", k.label),
        );
        // **`+ new map` is the first row, and on an empty kit it is the only one** — which is
        // §1.4's instruction-not-a-report, said by a row you can press rather than by a sentence.
        let mut maps = Vec::new();
        if kit.is_some() {
            maps.push(Row {
                left: "+ new map".to_owned(),
                right: "N".to_owned(),
                tone: if self.focus == Focus::Maps && self.map == 0 {
                    Tone::Selected
                } else {
                    Tone::Row
                },
            });
        }
        maps.extend(match kit {
            Some(k) => k
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
                        left: clip(&m.name, 16),
                        right,
                        tone: if selected { Tone::Selected } else { tone },
                    }
                })
                .collect::<Vec<_>>(),
            None => Vec::new(),
        });

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
            asking: self.ask.as_ref().map(|a| match a {
                Ask::Delete(c) => format!(
                    "delete `{}`? {} goes, and nothing here can bring it back.",
                    c.map,
                    c.path
                        .file_name()
                        .map_or_else(|| c.path.display().to_string(), |f| f
                            .to_string_lossy()
                            .into_owned())
                ),
                Ask::Quit => "quit emerge-mapper?".to_owned(),
            }),
            problem: self.problem.clone(),
            hint: self.hint().to_owned(),
        }
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
                self.current_kit()
                    .map_or_else(|| "NEW MAP".to_owned(), |k| format!("NEW MAP IN {}", k.label)),
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
                        format!("SETTINGS FOR {}", m.name),
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
            _ if self.editing => "type    Enter keep    Esc leave the field",
            Focus::Settings if self.creating.is_some() => {
                "up/down field    Enter edit    Tab panel    Ctrl+Enter make it    Esc cancel"
            }
            Focus::Settings => "up/down field    Enter edit    Tab panel    Esc quit",
            // **Only verbs that would do something right now.** `Enter` opens a map — so it is
            // not offered on a kit with none, nor on the `+ new` row where it makes instead.
            Focus::Kits if self.kit == 0 => {
                "up/down kit    Enter new kit    Tab panel    Esc quit"
            }
            Focus::Kits if self.current_kit().is_some_and(|k| k.maps.is_empty()) => {
                "up/down kit    Tab panel    N new kit    Esc quit"
            }
            Focus::Kits => "up/down kit    Tab panel    Enter open    N new kit    Esc quit",
            Focus::Maps if self.map == 0 => {
                "up/down map    Enter new map    Tab panel    Esc quit"
            }
            Focus::Maps => {
                "up/down map    Tab panel    Enter open    Delete remove    Esc quit"
            }
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
            ask: self.chooser.ask.clone(),
            swallowed: false,
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
const COL: f32 = 300.0;

/// How many columns stand side by side: kits, that kit's maps, that map's settings.
const COLS: f32 = 3.0;

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
    let content = COL * COLS + crate::chrome::PAD * (COLS - 1.0);
    let width = (content + crate::chrome::PAD * 3.0) * UI_SCALE;
    // Title, one row of columns, the message line and the hint — measured off a capture rather
    // than computed from font metrics, which would be a second layout engine. The slack is
    // deliberate and small: a kit with several maps grows the row, and a window that has to be
    // resized to see the last map is worse than one with a little air at the bottom.
    let height = 300.0 * UI_SCALE;
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
            // **Three columns, left to right, each one the contents of the selection beside it.**
            //
            // Reported at the keyboard: *"can we make it clearer that the settings refer to a map?
            // the hierarchy of the data structure isn't clear."* It was not: three panels of equal
            // weight, with the settings as a full-width footer under both lists, read as three
            // siblings — when a kit *contains* maps and a map *has* settings.
            //
            // Columns are that containment made spatial, and each header names its parent
            // (`MAPS IN emerge`, `SETTINGS FOR untitled_map`) so the chain is legible without
            // relying on position alone. It is also exactly the order `Tab` walks, which was
            // already true and previously invisible.
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
                row.spawn(panel(COL)).with_children(|p| {
                    p.spawn((header("SETTINGS"), SettingsHeader));
                    p.spawn((Node::default(), SettingsList));
                });
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
            if let Some(i) = chooser
                .catalog
                .kits
                .iter()
                .position(|k| &k.label == name)
            {
                chooser.kit = i + 1;
                chooser.map = Chooser::first_real(chooser.current_kit().map_or(0, |k| k.maps.len()));
            }
            chooser.focus = Focus::Kits;
        }
        New::Map(d) => {
            let dir = chooser
                .current_kit()
                .map(|k| k.dir.clone())
                .ok_or_else(|| "no kit selected".to_owned())?;
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
                .map_or_else(|| Chooser::first_real(chooser.catalog.kits.len()), |i| i + 1);
            chooser.map = want
                .and_then(|w| {
                    chooser
                        .current_kit()
                        .and_then(|k| k.maps.iter().position(|m| m.name == w))
                        .map(|i| i + 1)
                })
                .unwrap_or_else(|| {
                    Chooser::first_real(chooser.current_kit().map_or(0, |k| k.maps.len()))
                });
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
    // **A key the text handler already took is not read again.** One `Escape` is one press; see
    // `Chooser::swallowed` for the bug this closes.
    if std::mem::take(&mut chooser.swallowed) {
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
        if keyboard.just_pressed(KeyCode::Escape) || keyboard.just_pressed(KeyCode::KeyN) {
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
        if let Some(new) = chooser.creating.clone()
            && commit_new
        {
            chooser.problem = make_it(&mut chooser, &new).err();
            return;
        }
        // In the settings, `Enter` opens the highlighted row for typing.
        if chooser.focus == Focus::Settings {
            chooser.raw.clear();
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
        for verb in ["Enter open", "N new kit", "Esc quit"] {
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

// ------------------------------------------------------------------------------------------------
// The guide vocabulary
// ------------------------------------------------------------------------------------------------

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
                top: 40.0,
                ..default()
            });

        let want = |args: &Value, key: &str| -> Option<String> {
            args.get(key).and_then(Value::as_str).map(str::to_owned)
        };

        let on_kits = app.register_system(|_: In<Value>, c: Res<Chooser>| c.focus == Focus::Kits);
        let on_maps = app.register_system(|_: In<Value>, c: Res<Chooser>| c.focus == Focus::Maps);
        let on_settings =
            app.register_system(|_: In<Value>, c: Res<Chooser>| c.focus == Focus::Settings);
        let making_kit =
            app.register_system(|_: In<Value>, c: Res<Chooser>| matches!(c.creating, Some(New::Kit(_))));
        let making_map =
            app.register_system(|_: In<Value>, c: Res<Chooser>| matches!(c.creating, Some(New::Map(_))));
        let typing = app.register_system(|_: In<Value>, c: Res<Chooser>| c.editing);

        // **By name, and off the CATALOG rather than the draft** — the catalog is a description of
        // disk, so this answers "does it exist" and not "did somebody type it".
        let kit_exists = app.register_system(move |args: In<Value>, c: Res<Chooser>| {
            want(&args.0, "name").is_some_and(|n| c.catalog.kits.iter().any(|k| k.label == n))
        });
        let map_exists = app.register_system(move |args: In<Value>, c: Res<Chooser>| {
            want(&args.0, "name").is_some_and(|n| {
                c.catalog
                    .kits
                    .iter()
                    .any(|k| k.maps.iter().any(|m| m.name == n))
            })
        });
        let map_gone = app.register_system(move |args: In<Value>, c: Res<Chooser>| {
            want(&args.0, "name").is_some_and(|n| {
                !c.catalog
                    .kits
                    .iter()
                    .any(|k| k.maps.iter().any(|m| m.name == n))
            })
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
