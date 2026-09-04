//! **The crate boundary, enforced.**
//!
//! `bevy_fracture_modes` is a cell graph, a dense symmetric solver and two resources. It takes
//! `bevy` with every feature off but `std` — `bevy::app`, `bevy::ecs` and `bevy::math` are the
//! whole of what it names — and, optionally, `serde`. Nothing else, and in particular **not
//! `bevy_carnage`**: that crate owns the convex decomposition and composes this one, so a
//! dependency in this direction would be a cycle. `nalgebra`, `faer` and friends are refused
//! because the solver's determinism rests on a fixed schedule of operations this crate can read;
//! a library's pivoting is a branch it cannot.
//!
//! These two tests are the ratchet: cheap, and they fail at the door rather than at link time.
//! Widening the list is a design decision, so it should cost a deliberate edit here.

use std::path::{Path, PathBuf};

/// Everything `bevy_fracture_modes` is allowed to depend on.
const ALLOWED_DEPS: &[&str] = &["bevy", "serde"];

/// Crate names that would mean the boundary has been crossed, checked as substrings.
///
/// `bevy_carnage` is here because the layering runs the other way; `bevy_hanabi`/`wgpu` because they
/// would put blood back on the GPU, which is the one thing this crate exists not to do; `avian`,
/// `emerge` and `foundation_vs_slop` because they are the game.
const FORBIDDEN_DEP_MARKERS: &[&str] = &[
    "bevy_carnage", "bloodstain", "nalgebra", "faer", "ndarray", "rand", "bevy_hanabi", "wgpu", "avian",
    "emerge", "foundation_vs_slop",
];

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
        "bevy_fracture_modes must stay a leaf below the gore layer, but {} line(s) reference a \
         forbidden crate.\n  {}\n\n\
         `bevy_carnage` composes this crate, so a dependency the other way is a cycle. A linear \
         algebra library would put a pivoting branch in a solver whose determinism rests on a fixed \
         schedule this crate can read line by line.",
        offenders.len(),
        offenders.join("\n  ")
    );
}

#[test]
fn the_dependency_list_stays_closed() {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml"))
        .expect("bevy_fracture_modes must have a Cargo.toml");

    // Every table whose header ends in `dependencies]` and is not a dev table — so a target-gated
    // `[target.'cfg(..)'.dependencies]` is read too. Only the plain `[dependencies]` used to be, and
    // a real dependency behind a `cfg` would have passed this ratchet green. Dev tables are skipped
    // on purpose: a dev-dependency on the full `bevy` umbrella is what lets the windowed example
    // exist without reaching a consumer's dependency graph, and that is a different (and much less
    // alarming) conversation.
    let mut deps = String::new();
    let mut in_table = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_table = trimmed.ends_with("dependencies]") && !trimmed.ends_with("dev-dependencies]");
            continue;
        }
        if in_table {
            deps.push_str(line);
            deps.push('\n');
        }
    }
    assert!(!deps.trim().is_empty(), "bevy_fracture_modes must declare a [dependencies] table");
    let deps = deps.as_str();

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
            "bevy_fracture_modes declares `{name}`, which is not in its allowed set {ALLOWED_DEPS:?}.\n\
             Widening this is a design decision, not a convenience. If it is genuinely warranted, add \
             it to ALLOWED_DEPS in this test in the same commit, so the change is visible in review."
        );
        for marker in FORBIDDEN_DEP_MARKERS {
            assert!(
                !name.contains(marker),
                "bevy_fracture_modes declares `{name}` — see this file's header for why that name in \
                 particular is refused."
            );
        }
    }
}
