//! **The measurement cache, persisted** — so the STALE badge is truthful at startup instead of
//! only after the tab's first audit, and a session that changed nothing re-measures nothing.
//!
//! One RON file under the project's `target/` (gitignored; `cargo clean` losing a cache is the
//! correct cost of it being one). An entry is kept on load only when three things still hold: the
//! rig still exists, the manifest's `Rig` equals the one the report was measured under (a
//! hand-edited declared value invalidates exactly — no second hash scheme), and the GLB's bytes
//! hash to the report's fingerprint. Anything else is dropped without a complaint line: a cache is
//! not entitled to one.
//!
//! **No threads**, same as `anim_watch`: the load is synchronous at Startup (hashing all sixteen
//! GLBs is ~15 MB of FNV-1a, tens of milliseconds, once), and the save is a write-through on the
//! generation bump the reports already signal with.
//!
//! Known, accepted limitation: a GLB re-exported *while the editor was closed* misses the cache
//! (hash mismatch → dropped) and waits for the tab's audit-on-open to be measured — strictly
//! better than before, when the badge knew nothing at all until then.

use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::anim_watch::{BenchGeneration, BenchReports, RigReport};
use crate::project::Project;

/// Bumped when the cache's own shape changes. The `tool` field guards the *measurements'* meaning
/// ([`emerge_core::rigs::BENCH_TOOL_VERSION`]); this guards the container.
pub const CACHE_VERSION: u32 = 1;

/// The cache file, relative to the project root.
pub const CACHE_PATH: &str = "target/anim_bench_cache.ron";

#[derive(serde::Serialize, serde::Deserialize)]
struct CacheFile {
    version: u32,
    tool: u32,
    /// Keyed by rig name, like [`BenchReports`].
    entries: BTreeMap<String, CacheEntry>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CacheEntry {
    /// The whole manifest entry the report was measured under — equality on load is what makes
    /// invalidation exact.
    rig: emerge_core::rigs::Rig,
    report: RigReport,
}

/// The cache switch — `false` forces cold state, for tests that assert the queue path against a
/// developer machine whose `target/` holds a real cache. One code path, one switch.
#[derive(Resource)]
pub struct BenchCache {
    pub enabled: bool,
}

impl Default for BenchCache {
    fn default() -> Self {
        BenchCache { enabled: true }
    }
}

/// **The load logic, pure over the filesystem root** — what the Startup system wraps and the
/// tests drive directly. Returns the reports that survive every check.
fn warm_reports(root: &std::path::Path) -> BTreeMap<String, RigReport> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(root.join(CACHE_PATH)) else {
        return out; // no cache yet — the first session writes one
    };
    let Ok(file) = ron::from_str::<CacheFile>(&text) else {
        return out; // an older/foreign shape re-measures; it does not complain
    };
    if file.version != CACHE_VERSION || file.tool != emerge_core::rigs::BENCH_TOOL_VERSION {
        return out;
    }
    let Ok(manifest_text) = std::fs::read_to_string(root.join("assets/emerge/rigs.ron")) else {
        return out;
    };
    let Ok(rigs) = emerge_core::rigs::Rigs::parse(&manifest_text) else {
        return out;
    };
    for (name, entry) in file.entries {
        let Some(current) = rigs.get(&name) else {
            continue; // the rig was deleted since
        };
        if entry.rig != *current {
            continue; // the manifest changed under the report — declared values included
        }
        let Some(recorded) = entry.report.fingerprint else {
            continue; // an error report is cheap to recompute; never resurrect one
        };
        let mesh = root.join("assets").join(&current.mesh);
        let Ok(bytes) = std::fs::read(&mesh) else {
            continue;
        };
        if emerge_core::glb::fnv1a(&bytes) != recorded {
            continue; // the asset was re-exported while the editor was closed
        }
        out.insert(name, entry.report);
    }
    out
}

/// Startup: warm [`BenchReports`] from disk. Parses the manifest for validation only — the tab's
/// lazy `load_on_entry` still owns `BenchState`.
pub(crate) fn load_bench_cache(
    project: Option<Res<Project>>,
    cache: Option<Res<BenchCache>>,
    mut reports: ResMut<BenchReports>,
    mut generation: ResMut<BenchGeneration>,
) {
    let Some(project) = project else { return };
    if !cache.is_some_and(|c| c.enabled) {
        return;
    }
    let warmed = warm_reports(&project.root);
    if warmed.is_empty() {
        return;
    }
    reports.by_rig.extend(warmed);
    // One bump: the badge repaints truthfully at startup, and `load_on_entry`/`check_all` skip
    // the warmed rigs for free (they test `contains_key`).
    generation.0 = generation.0.wrapping_add(1);
}

/// Write-through on every real report change. Only once the manifest is loaded: before tab entry
/// the only bump is the load's own, and rewriting the file with its own contents would be noise —
/// while an EMPTY write after `invalidate()` is correct, because the reports no longer describe
/// disk. State the invariant so nobody "fixes" it into a truncation.
pub(crate) fn save_bench_cache(
    project: Option<Res<Project>>,
    cache: Option<Res<BenchCache>>,
    bench: Option<Res<crate::anim_tab::BenchState>>,
    reports: Res<BenchReports>,
) {
    let Some(project) = project else { return };
    if !cache.is_some_and(|c| c.enabled) {
        return;
    }
    let Some(bench) = bench else { return };
    let Some(rigs) = bench.rigs.as_ref() else {
        return;
    };
    let mut entries = BTreeMap::new();
    for (name, report) in &reports.by_rig {
        if report.fingerprint.is_none() {
            continue; // unreadable-file reports are not worth resurrecting
        }
        let Some(rig) = rigs.get(name) else {
            continue;
        };
        entries.insert(
            name.clone(),
            CacheEntry {
                rig: rig.clone(),
                report: report.clone(),
            },
        );
    }
    let file = CacheFile {
        version: CACHE_VERSION,
        tool: emerge_core::rigs::BENCH_TOOL_VERSION,
        entries,
    };
    let Ok(text) = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()) else {
        return;
    };
    let path = project.root.join(CACHE_PATH);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Atomic via a pid-suffixed temp + rename: two concurrently-stepped test apps sharing a root
    // must not share one temp name. A failed write leaves the old cache, which is merely stale —
    // the load's checks make stale harmless.
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if std::fs::write(&tmp, &text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// A disposable project root with the real manifest and the real valkyrie GLB — the
    /// `anim_tab::write_back_tests` recipe.
    fn temp_project() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "anim_cache_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let ws = workspace_root();
        std::fs::create_dir_all(dir.join("assets/emerge")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir_all(dir.join("assets/characters")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::copy(
            ws.join("assets/emerge/rigs.ron"),
            dir.join("assets/emerge/rigs.ron"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        std::fs::copy(
            ws.join("assets/characters/valkyrie.glb"),
            dir.join("assets/characters/valkyrie.glb"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        dir
    }

    fn valkyrie_report_and_rig(
        root: &std::path::Path,
    ) -> (RigReport, emerge_core::rigs::Rig) {
        let text = std::fs::read_to_string(root.join("assets/emerge/rigs.ron"))
            .unwrap_or_else(|e| panic!("{e}"));
        let rigs = emerge_core::rigs::Rigs::parse(&text).unwrap_or_else(|e| panic!("{e}"));
        let rig = rigs
            .get("valkyrie")
            .cloned()
            .unwrap_or_else(|| panic!("no valkyrie"));
        let report = crate::anim_watch::measure_rig(root, &rig);
        (report, rig)
    }

    fn write_cache(root: &std::path::Path, entries: BTreeMap<String, CacheEntry>) {
        let file = CacheFile {
            version: CACHE_VERSION,
            tool: emerge_core::rigs::BENCH_TOOL_VERSION,
            entries,
        };
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
            .unwrap_or_else(|e| panic!("{e}"));
        let path = root.join(CACHE_PATH);
        std::fs::create_dir_all(path.parent().unwrap_or_else(|| panic!("no parent")))
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(path, text).unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn a_cached_report_round_trips_and_warms_a_fresh_session() {
        let root = temp_project();
        let (report, rig) = valkyrie_report_and_rig(&root);
        assert_eq!(report.slots.len(), 6);
        let mut entries = BTreeMap::new();
        entries.insert("valkyrie".to_owned(), CacheEntry { rig, report: report.clone() });
        write_cache(&root, entries);
        let warmed = warm_reports(&root);
        let back = warmed
            .get("valkyrie")
            .unwrap_or_else(|| panic!("nothing warmed"));
        assert!(*back == report, "the warmed report must equal the measured one");
    }

    #[test]
    fn a_changed_asset_a_changed_manifest_or_a_version_bump_drops_the_entry() {
        // (a) A flipped byte in the GLB: the fingerprint no longer matches.
        let root = temp_project();
        let (report, rig) = valkyrie_report_and_rig(&root);
        let mut entries = BTreeMap::new();
        entries.insert(
            "valkyrie".to_owned(),
            CacheEntry { rig: rig.clone(), report: report.clone() },
        );
        write_cache(&root, entries);
        let glb_path = root.join("assets/characters/valkyrie.glb");
        let mut bytes = std::fs::read(&glb_path).unwrap_or_else(|e| panic!("{e}"));
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(&glb_path, bytes).unwrap_or_else(|e| panic!("{e}"));
        assert!(warm_reports(&root).is_empty(), "a re-exported GLB must drop the entry");

        // (b) A hand-edited declared value: the stored Rig no longer equals the manifest's.
        let root = temp_project();
        let (report, rig) = valkyrie_report_and_rig(&root);
        let mut stale_rig = rig.clone();
        stale_rig.scale = 1.0;
        let mut entries = BTreeMap::new();
        entries.insert("valkyrie".to_owned(), CacheEntry { rig: stale_rig, report });
        write_cache(&root, entries);
        assert!(warm_reports(&root).is_empty(), "a manifest edit must drop the entry");

        // (c) A cache-version bump discards the whole file.
        let root = temp_project();
        let (report, rig) = valkyrie_report_and_rig(&root);
        let mut entries = BTreeMap::new();
        entries.insert("valkyrie".to_owned(), CacheEntry { rig, report });
        let file = CacheFile {
            version: CACHE_VERSION + 1,
            tool: emerge_core::rigs::BENCH_TOOL_VERSION,
            entries,
        };
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir_all(root.join("target")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(root.join(CACHE_PATH), text).unwrap_or_else(|e| panic!("{e}"));
        assert!(warm_reports(&root).is_empty(), "a version bump must drop the file");
    }
}
