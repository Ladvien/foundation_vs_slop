//! **The crate boundary, enforced.**
//!
//! `emerge-core` exists so that the constraint IR, the solvers, WFC and the seeded RNG can be consumed
//! by the game, by the offline search, and by a standalone editor without any of them agreeing on a
//! renderer. That property was a *comment* for months — `ir.rs` has said "Nothing here imports
//! `bevy::`" since long before there was a crate — and a comment cannot fail a build.
//!
//! Adding `bevy` here would compile fine and nobody would notice until the editor tried to link it.
//! These two tests are the ratchet, in the spirit of `tests/determinism_lint.rs` and
//! `tests/panic_budget.rs`: cheap, GPU-free, and they fail at the door.
//!
//! Widening the dependency list is a design decision (see `docs/2026-08-03-emerge-mapper-plan.md`), so it
//! should cost an argument and a deliberate edit here — not a passing `cargo build`.
//!
//! # Which test actually catches what
//!
//! Both were verified by introducing a violation and watching them fail, and the experiment was
//! informative: writing `bevy::math::Vec2` in a source file **does not reach these tests at all** —
//! the crate simply fails to compile, because `bevy` is not a dependency. The compiler is the first
//! line of defence and it is a good one.
//!
//! So the real pairing is: [`the_dependency_list_stays_closed`] is the one that bites, because adding
//! the dependency is the only way to make an engine reference compile. [`no_source_file_reaches_for_an_engine`]
//! is the backstop for the case where someone adds the dep *and* edits `ALLOWED_DEPS` to match —
//! it names the file and line that motivated it, which a manifest diff cannot.

use std::path::{Path, PathBuf};

/// Everything `emerge-core` is allowed to depend on. Data and arithmetic, nothing that draws.
///
/// # `det_rng`, and why it widened nothing
///
/// The seeded RNG used to be `emerge-core/src/rng.rs`; it was lifted into a sibling crate so a
/// permissively-licensed consumer could depend on the generator without taking this crate, and
/// `emerge-core` re-exports it (`pub use det_rng as rng`) so no call site moved.
///
/// It is on this list because **the dependency surface did not actually grow**: `det_rng`'s own
/// manifest declares `rand` and `rand_chacha` and nothing else, both of which are already here. The
/// same code is reachable at a different address. It also carries its own `tests/leaf.rs`, so the
/// boundary is policed on that side rather than taken on trust from here.
///
/// That is the argument this list is supposed to cost. A dependency that pulled in anything not
/// already on this line would need a different one.
const ALLOWED_DEPS: &[&str] = &["serde", "serde_json", "ron", "rand", "rand_chacha", "det_rng"];

/// Crate names that would mean the boundary has been crossed, checked as substrings so
/// `bevy_math`/`bevy_ecs` are caught as readily as `bevy`.
const FORBIDDEN_DEP_MARKERS: &[&str] = &["bevy", "avian", "wgpu", "winit"];

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

/// Strip `//` line comments and `/* */` blocks so a doc comment *about* Bevy — of which this crate has
/// several, deliberately — is not mistaken for an import of it.
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
fn no_source_file_reaches_for_an_engine() {
    let src = crate_root().join("src");
    let mut files = Vec::new();
    rust_sources(&src, &mut files);
    assert!(
        files.len() >= 10,
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
        "emerge-core must stay engine-free, but {} line(s) reference an engine crate.\n  {}\n\n\
         This crate is what lets the game, the headless search and the standalone editor share the \
         placement stack without agreeing on a renderer. If a type genuinely needs to cross the \
         boundary, the answer is a plain-data type here and a conversion on the far side — not a \
         dependency. See docs/2026-08-03-emerge-mapper-plan.md.",
        offenders.len(),
        offenders.join("\n  ")
    );
}

#[test]
fn the_dependency_list_stays_closed() {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml"))
        .expect("emerge-core must have a Cargo.toml");

    // Only the `[dependencies]` table — a dev-dependency on something heavier would be a different
    // (and much less alarming) conversation.
    let deps = manifest
        .split("[dependencies]")
        .nth(1)
        .expect("emerge-core must declare a [dependencies] table");
    let deps = deps.split("\n[").next().unwrap_or(deps);

    for line in deps.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `.` is a separator too, so an inherited dependency written as the dotted key
        // `serde.workspace = true` reads as `serde` rather than as a crate called
        // `serde.workspace`. A crate name cannot contain a dot, so nothing real is truncated —
        // and the check keeps biting on the part that matters, the name before it.
        let name = line.split(['=', ' ', '.']).next().unwrap_or("");
        // A dependency line is `name = ...` or `name.workspace = true`. Anything else inside this
        // table is a CONTINUATION of a multi-line value — a feature array's entries, a closing
        // bracket — and is not a crate name. Reading one as a crate name produced a genuinely
        // baffling failure ("declares `\"bevy_asset\",`"), so continuations are skipped by shape.
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            continue;
        }
        assert!(
            ALLOWED_DEPS.contains(&name),
            "emerge-core declares `{name}`, which is not in its allowed set {ALLOWED_DEPS:?}.\n\
             Widening this is a design decision, not a convenience — see \
             docs/2026-08-03-emerge-mapper-plan.md. If it is genuinely warranted, add it to ALLOWED_DEPS in \
             this test in the same commit, so the change is visible in review."
        );
        for marker in FORBIDDEN_DEP_MARKERS {
            assert!(
                !name.contains(marker),
                "emerge-core declares `{name}` — the crate exists precisely so it does not depend on \
                 an engine."
            );
        }
    }
}
