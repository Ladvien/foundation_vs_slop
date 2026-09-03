//! **The crate boundary, enforced.**
//!
//! `bevy_viscera` is a solver and a mesh builder. It takes `bevy` — trimmed to the asset, mesh and log
//! features, because it hands back a `Mesh` and is a plugin — and nothing else.
//!
//! The name that is *not* here matters most: **`bloodstain`**. A rope solver has no business knowing
//! about rheology, hematocrit or drying, and the moment this crate can name a blood type it stops
//! being a thing a caller can use for tentacles, cables or hair. Nor does it take an RNG crate: the
//! digest this crate exists to print would then be reproducible only for as long as somebody else's
//! algorithm was.
//!
//! These two tests are the ratchet: cheap, and they fail at the door rather than at link time.
//! Widening the list is a design decision, so it should cost a deliberate edit here.

use std::path::{Path, PathBuf};

/// Everything `bevy_viscera` is allowed to depend on.
const ALLOWED_DEPS: &[&str] = &["bevy"];

/// Crate names that would mean the boundary has been crossed, checked as substrings in source so a
/// stray `use` is caught as readily as a manifest line.
///
/// `rand` is deliberately absent from this list even though the crate must never take it: the word is
/// a substring of `strand`, so it would fire on every line of `src/strand.rs`. `ALLOWED_DEPS` above is
/// what keeps it out, and that is the check that can actually see a dependency.
const FORBIDDEN_DEP_MARKERS: &[&str] = &[
    "bloodstain",
    "bevy_carnage",
    "bevy_wetmap",
    "avian",
    "wgpu",
    "winit",
    "emerge",
    "foundation_vs_slop",
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

/// Strip `//` line comments and `/* */` blocks so a doc comment *about* a crate — of which this one
/// has several, deliberately — is not mistaken for an import of it.
///
/// Deliberately simple: it does not track string literals, so a `"//"` inside a string would end the
/// line early. That is acceptable here because the only thing this feeds is a substring search for
/// crate names, and an over-eager strip can only ever cause a false *pass* on a line that already
/// contains no dependency.
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
fn no_source_file_reaches_past_the_engine() {
    let src = crate_root().join("src");
    let mut files = Vec::new();
    rust_sources(&src, &mut files);
    assert!(
        files.len() >= 8,
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
        "bevy_viscera must depend on nothing but bevy, but {} line(s) reference another crate.\n  \
         {}\n\nThis crate is a rope solver. It is reusable for guts, tentacles, cables and hair \
         precisely because it cannot name any of them — and it must never learn what blood is.",
        offenders.len(),
        offenders.join("\n  ")
    );
}

#[test]
fn the_dependency_list_stays_closed() {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml"))
        .expect("bevy_viscera must have a Cargo.toml");

    // Only the `[dependencies]` table — the windowed example's dev-dependency on the full render
    // stack is a different (and much less alarming) conversation.
    let deps = manifest
        .split("[dependencies]")
        .nth(1)
        .expect("bevy_viscera must declare a [dependencies] table");
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
        if name.is_empty()
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            continue;
        }
        assert!(
            ALLOWED_DEPS.contains(&name),
            "bevy_viscera declares `{name}`, which is not in its allowed set {ALLOWED_DEPS:?}.\n\
             Widening this is a design decision, not a convenience. If it is genuinely warranted, \
             add it to ALLOWED_DEPS in this test in the same commit, so the change is visible in \
             review."
        );
        for marker in FORBIDDEN_DEP_MARKERS {
            assert!(
                !name.contains(marker),
                "bevy_viscera declares `{name}` — the crate exists precisely so it does not know \
                 about that."
            );
        }
    }
}
