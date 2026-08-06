//! **The manifest agrees with the assets it describes.**
//!
//! `assets/emerge/rigs.ron` carries numbers that were measured off the GLBs. Before
//! `emerge_core::clips` existed there was no way to re-check them, so `docs/animation.md` recorded the
//! measuring as *"a manual offline step, not a repo tool"* and the numbers quietly aged: an artist
//! re-exports a rig, the clip order or a cycle length shifts, and the game keeps animating to the old
//! table with no error anywhere — a creature that skates or drifts out of phase, which reads as "the
//! animation feels bad" rather than as a stale constant.
//!
//! This is the check that closes that. It re-measures every gait in the manifest from the file the
//! manifest names, and fails when they part company.

use std::path::{Path, PathBuf};

use emerge_core::glb::Glb;
use emerge_core::rig_check::{self, Level};
use emerge_core::rigs::Rigs;

/// The workspace root — tests run with the crate directory as cwd.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("emerge-core should sit two levels below the workspace root"))
        .to_path_buf()
}

fn manifest() -> Rigs {
    let path = root().join("assets/emerge/rigs.ron");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    Rigs::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_manifest_is_valid() {
    let rigs = manifest();
    assert!(!rigs.rigs.is_empty(), "the manifest describes no rigs");
}

/// **Every rig agrees with the asset it names** — the four checks, from the one shared policy.
///
/// This is `emerge_core::rig_check::check_rig`, the exact code path the editor's animation bench
/// runs, so a red here reproduces locally as the same words in the bench and vice versa. The finding
/// text carries the remedy, which is what makes a finding a usable panic message.
///
/// The old shape of this file — three per-check tests with their own loops, thresholds and a private
/// copy of the 1.13 figurine scale — silently skipped the FK checks on any rig without a node named
/// `Root` or `foot_l`. The shared policy reports that loudly instead (a gait rig owes its anchors),
/// reads each rig's `scale` and anchors from the manifest, and is the same rule for the editor and
/// for CI because it is the same code.
#[test]
fn every_rig_agrees_with_the_asset_it_names() {
    let mut bad = Vec::new();
    for (name, rig) in &manifest().rigs {
        let path = root().join("assets").join(&rig.mesh);
        let (glb, hash) = Glb::open_fingerprinted(&path)
            .unwrap_or_else(|e| panic!("rig `{name}`: {}: {e}", path.display()));
        // CI tightens exactly when the editor does — same staleness, same policy.
        let current = rig_check::staleness(rig, hash) == rig_check::Staleness::Current;
        let report = rig_check::check_rig(&glb, rig, current);
        for f in &report.findings {
            if f.level == Level::Bad {
                bad.push(format!("rig `{name}`: {}", f.text));
            }
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}
