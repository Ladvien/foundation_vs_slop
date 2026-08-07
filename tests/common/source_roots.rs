//! The roots every source lint walks.
//!
//! `src/` used to be the whole answer, and it stopped being one the day the ORCA solver, the QD kernel
//! and the screenshot helper became crates. None of that code changed — so a lint that stopped looking
//! at it would not go red, it would go **quiet**, which is the failure mode this repo has already met
//! once: extracting the animation layer into `crates/emerge-anim` took its 21 tests out of the gate
//! and nothing went red, because bare `cargo test` compiles no test target under `crates/` when the
//! workspace has a root package (see the note in `.github/workflows/ci.yml`).
//!
//! The list is shared rather than copied into each lint for the same reason: three copies drift, and
//! the drift is silent.
//!
//! # What is deliberately absent
//!
//! `crates/emerge-*` are **not** here, and that is a known gap rather than an oversight. Those four
//! crates have never been under these lints, so adding them is a measurement (how many unannotated
//! sorts? how many panic sites?) and a budget decision — not a one-line edit. It belongs in its own
//! change, with the numbers in the commit message.

use std::path::{Path, PathBuf};

/// Every tree the source lints scan. Add a crate here in the same commit that creates it.
pub const SCANNED_ROOTS: &[&str] = &[
    "src",
    "crates/bevy_orca/src",
    "crates/map_elites/src",
    "crates/bevy_devshot/src",
    "crates/bevy_stigmergy/src",
    "crates/bevy_light_grid/src",
];

/// Every `.rs` file under [`SCANNED_ROOTS`], sorted, at the workspace-relative path the lints'
/// exemption lists key on (`cargo test` runs the root package's test binaries with the cwd at the
/// workspace root).
///
/// A missing root is a **hard failure, not a skip**. A renamed or deleted crate directory would
/// otherwise silently shrink every lint's scope while all of them stayed green — the exact shape of
/// bug this module exists to prevent.
pub fn scanned_sources() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in SCANNED_ROOTS {
        let dir = Path::new(root);
        assert!(
            dir.is_dir(),
            "source-lint root `{root}` does not exist. If a crate moved, point this list at its new \
             home rather than dropping the entry — the code it covers did not stop needing the lint."
        );
        rust_files(dir, &mut out);
    }
    out.sort();
    assert!(
        !out.is_empty(),
        "the source lints found no files — is the test's working dir the workspace root?"
    );
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}
