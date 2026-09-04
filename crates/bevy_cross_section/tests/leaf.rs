//! **The crate boundary, enforced.**
//!
//! `bevy_cross_section` is a thickness table, a depth query, a procedural strip and one bake system.
//! It takes `bevy` — trimmed to the asset arena, the image and mesh types, `StandardMaterial` and
//! three `wgpu` type names — and `bloodstain`, for the one frozen hash and the spectral colour of a
//! wet muscle face. Nothing else.
//!
//! **`bevy_carnage` is forbidden by name because the layering runs the other way**: it composes this
//! crate, annotating its caps and taking the material back. A dependency in this direction would be
//! a cycle. `bevy_hanabi` and `wgpu` would put the bake on the GPU, which is the one thing a hashable
//! strip cannot allow; `avian`, `emerge` and `foundation_vs_slop` are the game.
//!
//! These two tests are the ratchet: cheap, and they fail at the door rather than at link time.
//! Widening the list is a design decision, so it should cost a deliberate edit here.

use std::path::{Path, PathBuf};

/// Everything `bevy_cross_section` is allowed to depend on.
const ALLOWED_DEPS: &[&str] = &["bevy", "bloodstain", "serde"];

/// Crate names that would mean the boundary has been crossed, checked as substrings.
///
/// `bevy_carnage` is here because the layering runs the other way; `bevy_hanabi`/`wgpu` because they
/// would put blood back on the GPU, which is the one thing this crate exists not to do; `avian`,
/// `emerge` and `foundation_vs_slop` because they are the game.
const FORBIDDEN_DEP_MARKERS: &[&str] =
    &["bevy_carnage", "bevy_hanabi", "wgpu", "avian", "emerge", "foundation_vs_slop"];

fn crate_root() -> PathBuf {
    // Cargo runs a test binary with the cwd set to its own package root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Strip `//` line comments and `/* */` blocks so a doc comment *about* a forbidden crate — of which
/// this crate has several, deliberately, since the layering argument names `bevy_carnage` — is not
/// mistaken for an import of it.
///
/// Deliberately simple: it does not track string literals, so a `"//"` inside a string would end the
/// line early. That is acceptable because the only thing this feeds is a substring search for crate
/// names, and an over-eager strip can only cause a false *pass* on a line that already contains no
/// dependency.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_block = false;
    for line in src.lines() {
        let mut rest = line;
        loop {
            if in_block {
                match rest.find("*/") {
                    Some(end) => {
                        in_block = false;
                        rest = &rest[end + 2..];
                    }
                    None => break,
                }
            } else {
                let line_at = rest.find("//");
                let block_at = rest.find("/*");
                match (line_at, block_at) {
                    (Some(l), b) if b.is_none_or(|b| l < b) => {
                        out.push_str(&rest[..l]);
                        break;
                    }
                    (_, Some(b)) => {
                        out.push_str(&rest[..b]);
                        in_block = true;
                        rest = &rest[b + 2..];
                    }
                    _ => {
                        out.push_str(rest);
                        break;
                    }
                }
            }
        }
        out.push('\n');
    }
    out
}

#[test]
fn no_source_file_reaches_across_the_layering() {
    let src = crate_root().join("src");
    let mut files = Vec::new();
    rust_sources(&src, &mut files);
    assert!(
        files.len() >= 4,
        "expected to scan the whole crate, found only {} file(s) — has the layout moved?",
        files.len()
    );

    let mut offenders = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let code = strip_comments(&text);
        for (n, line) in code.lines().enumerate() {
            for marker in FORBIDDEN_DEP_MARKERS {
                if line.contains(marker) {
                    offenders.push(format!(
                        "{}:{} — {}",
                        path.strip_prefix(crate_root()).unwrap_or(path).display(),
                        n + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "bevy_cross_section must stay below the gore layer and off the GPU, but {} line(s) reference a \
         forbidden crate.\n  {}\n\n\
         `bevy_carnage` composes this crate, so a dependency the other way inverts the layering — \
         that is why `src/uv.rs` reimplements Moller-Trumbore rather than importing it. A compute or \
         physics dependency would put the authority back on the GPU, which is the one thing this \
         crate exists not to do.",
        offenders.len(),
        offenders.join("\n  ")
    );
}

#[test]
fn the_dependency_list_stays_closed() {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml"))
        .expect("bevy_cross_section must have a Cargo.toml");

    // Only the `[dependencies]` table — a dev-dependency on the full `bevy` umbrella is what lets the
    // windowed example exist without reaching a consumer's dependency graph, and that is a different
    // (and much less alarming) conversation.
    let deps = manifest
        .split("[dependencies]")
        .nth(1)
        .expect("bevy_cross_section must declare a [dependencies] table");
    let deps = deps.split("\n[").next().unwrap_or(deps);

    for line in deps.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `.` is a separator too, so an inherited dependency written as the dotted key
        // `serde.workspace = true` reads as `serde`. A crate name cannot contain a dot, so nothing
        // real is truncated.
        let name = line.split(['=', ' ', '.']).next().unwrap_or("");
        // A dependency line is `name = ...` or `name.workspace = true`. Anything else inside this
        // table is a CONTINUATION of a multi-line value — a feature array's entries, a closing
        // bracket — and is not a crate name.
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            continue;
        }
        assert!(
            ALLOWED_DEPS.contains(&name),
            "bevy_cross_section declares `{name}`, which is not in its allowed set {ALLOWED_DEPS:?}.\n\
             Widening this is a design decision, not a convenience. If it is genuinely warranted, add \
             it to ALLOWED_DEPS in this test in the same commit, so the change is visible in review."
        );
        for marker in FORBIDDEN_DEP_MARKERS {
            assert!(
                !name.contains(marker),
                "bevy_cross_section declares `{name}` — see this file's header for why that name in \
                 particular is refused."
            );
        }
    }
}
