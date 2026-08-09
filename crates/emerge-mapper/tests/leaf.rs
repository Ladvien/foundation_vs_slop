//! **The crate's boundary, as a test rather than a comment.**
//!
//! Root `CLAUDE.md` asks every mirrored crate for a dependency ratchet *"so widening the boundary
//! costs a deliberate edit rather than a passing build"*. This crate did not have one, and it is the
//! crate where it matters most: it is the only place in the workspace that speaks HTTP. `vlm.rs`
//! reaches a vision model over `ureq`, and the argument that this is safe rests entirely on the editor
//! never shipping. That argument is worth exactly as much as the list below is enforced.
//!
//! # Two things this deliberately does not do
//!
//! It does not forbid `bevy` — this crate *is* a Bevy application, which is the difference between it
//! and `emerge-core`'s `engine_free.rs`. And it does not check the game's own manifest: the editor is
//! not a dependency of the game, and `cargo tree -i emerge-mapper` is what would notice if that
//! changed.
//!
//! # Parsing
//!
//! By hand, over the `[dependencies]` section only. `docs/2026-08-08-handoff.md` records that the
//! existing ratchets split on `= . ' '` and have tripped on multi-line `features = [...]` arrays —
//! which this manifest has, on `bevy` — so this reads a dependency name as *the first token of a line
//! at zero indentation inside the section*, and ignores continuation lines outright.

use std::collections::BTreeSet;

/// Every crate `emerge-mapper` may depend on directly, and why it is allowed.
///
/// Adding a row here is the deliberate edit. Removing the need for one is better.
const ALLOWED: &[(&str, &str)] = &[
    ("emerge-core", "the engine-free schema, solvers and validation"),
    ("emerge-bevy", "the one spawner, so a map cannot look one way here and another in the game"),
    ("emerge-anim", "the game's real pose blender, so the bench previews what the game plays"),
    ("bevy_devshot", "the one capture rig"),
    ("bevy", "this is a Bevy application"),
    ("ron", "map and library serialization"),
    ("serde", "derives on every schema type this crate reads and writes"),
    ("serde_json", "the VLM chat request and reply bodies"),
    ("ureq", "the VLM transport — one blocking POST from a task-pool thread"),
    ("base64", "data-URI image parts for the booth renders"),
    ("image", "encodes the booth's RGBA readback to PNG"),
    ("arboard", "Cmd+C on the detail pane"),
];

fn declared_dependencies(manifest: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut inside = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == "[dependencies]";
            continue;
        }
        if !inside || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // A continuation line inside a multi-line table or array is indented; a dependency is not.
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let Some((name, _)) = trimmed.split_once('=') else {
            continue;
        };
        // `ron.workspace = true` — the dotted form names the crate before the dot.
        let name = name.trim().split('.').next().unwrap_or("").trim();
        if !name.is_empty() {
            out.insert(name.to_owned());
        }
    }
    out
}

#[test]
fn the_editor_depends_on_nothing_it_has_not_argued_for() {
    let manifest = include_str!("../Cargo.toml");
    let declared = declared_dependencies(manifest);
    assert!(
        !declared.is_empty(),
        "parsed no dependencies at all — the parser broke, not the manifest"
    );
    let allowed: BTreeSet<&str> = ALLOWED.iter().map(|(n, _)| *n).collect();
    let extra: Vec<&String> = declared.iter().filter(|d| !allowed.contains(d.as_str())).collect();
    assert!(
        extra.is_empty(),
        "emerge-mapper declares {extra:?}, which nothing in this test argues for. Widening the \
         boundary is fine — add the crate to ALLOWED with the sentence that says why it earns its \
         place, and that sentence is what a reviewer reads."
    );
}

/// The list is a claim about the manifest, so a stale row is a lie in the other direction.
#[test]
fn the_allowed_list_has_no_rows_the_manifest_dropped() {
    let declared = declared_dependencies(include_str!("../Cargo.toml"));
    let stale: Vec<&str> = ALLOWED
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !declared.contains(*n))
        .collect();
    assert!(
        stale.is_empty(),
        "ALLOWED still argues for {stale:?}, which the manifest no longer declares. A ratchet that \
         permits more than exists stops being a description of anything."
    );
}

/// Every row says why. A bare name is the comment this test replaced.
#[test]
fn every_allowed_dependency_carries_its_argument() {
    for (name, why) in ALLOWED {
        assert!(
            why.len() > 10,
            "`{name}` is allowed with no argument for it. The reason is the point — a list of names \
             is what the manifest already is."
        );
    }
}
