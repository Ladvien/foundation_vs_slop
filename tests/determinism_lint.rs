//! **Source lint: every sort in the sim must declare its determinism contract.**
//!
//! GPU-free, no `App` — this runs in the `cargo test` hard gate, so it blocks on every push.
//!
//! # Why this exists
//!
//! The gameplay sim is bit-reproducible only because ~dozens of sites that iterate an ECS query impose a
//! stable order on it first. ECS query order is **not stable across `App` instances** (GLB scene-child
//! instantiation + entity-id reuse permute it), so any ordering decision that falls through to it is
//! irreproducible. That single mistake, in various costumes, is the whole of G0/G0b/G0c.
//!
//! Comments were the only enforcement, and comments do not fail. Three separate sites —
//! `almond_water::almond_water_effect`, `enemy::smiley_defense`, and the ORCA neighbour sort — carried
//! comments *asserting* a total order while keying on a prefix of the value (position bits). All three were
//! wrong in the same way: crabs `clamp_to_patch`-ed against a wall hold BIT-IDENTICAL coordinates, so the
//! key tied and `sort_unstable` resolved it by exactly the query order the sort existed to erase. Measured:
//! 6 fully-tied pairs at one tick on held-in world `0xA11CE`. Each site documented the trap it fell into.
//!
//! # The contract
//!
//! Every sort in `src/` must pick one, explicitly:
//!
//! * [`sort_total!`] — the key is a **total** order (no two elements can produce it). Checked at runtime
//!   under `test-harness`/debug: a tie panics naming the file, line, and duplicated key. Use this whenever
//!   order is load-bearing — a greedy loop, a `take(n)` budget, a shared RNG draw or counter, a clamped
//!   accumulate, a last-writer-wins write, a lethal pick.
//! * [`util::sort_value_canonical`] — ties are legitimate because tied elements are **interchangeable**
//!   (sort by the WHOLE value, so a tie means they are identical). The claim is on the caller.
//! * A raw `sort*` with a `SORT-OK: <reason>` comment within the preceding 4 lines — for sorts whose input
//!   never comes from an ECS query (seeded generators, fixed constant tables, pure geometry).
//!
//! An unannotated raw sort fails this test. That is the point: the author must state which case they are in,
//! and "I did not think about it" is not one of the three.

use std::path::{Path, PathBuf};

/// `util.rs` defines the sanctioned helpers, so its own `sort_unstable_by_key` calls ARE the primitives.
const EXEMPT_FILES: &[&str] = &["src/util.rs"];

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

#[test]
fn every_sort_declares_its_determinism_contract() {
    let mut files = Vec::new();
    rust_files(Path::new("src"), &mut files);
    files.sort();
    assert!(!files.is_empty(), "found no sources under src/ — is the test's working dir the crate root?");

    let mut offenders: Vec<String> = Vec::new();
    for path in &files {
        let rel = path.to_string_lossy().replace('\\', "/");
        if EXEMPT_FILES.contains(&rel.as_str()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let lines: Vec<&str> = text.lines().collect();

        // Everything from a `#[cfg(test)]` module (or an explicit `// determinism-lint: off` marker) to
        // EOF is test-only: its inputs are hand-built Vecs, not ECS queries, so the contract does not
        // apply. Robust to `#[cfg(any(test, ...))]`, `#[cfg(all(test, ...))]`, and internal spacing — not
        // just the bare `#[cfg(test)]` the old `starts_with` matched (Finding C, 2026-07-19 review).
        let test_mod = lines.iter().position(|l| {
            let t = l.trim_start();
            (t.starts_with("#[cfg(") && cfg_enables_test(t)) || t.starts_with("// determinism-lint: off")
        });

        for (i, line) in lines.iter().enumerate() {
            if test_mod.is_some_and(|t| i >= t) {
                continue;
            }
            let code = line.split("//").next().unwrap_or("");
            let is_sort = code.contains(".sort_unstable_by_key(")
                || code.contains(".sort_by_key(")
                || code.contains(".sort_unstable_by(")
                || code.contains(".sort_by(")
                || code.contains(".sort_unstable()")
                || code.contains(".sort()");
            if !is_sort {
                continue;
            }
            // Annotated? Look back a few lines for the escape hatch.
            let lo = i.saturating_sub(4);
            let annotated = lines[lo..i].iter().any(|l| l.contains("SORT-OK:"));
            if !annotated {
                offenders.push(format!("  {rel}:{}  {}", i + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "\n{} unannotated raw sort(s) — each must declare its determinism contract:\n\n{}\n\n\
         Pick one:\n  \
           * `sort_total!(&mut v, |x| key)` — the key is a TOTAL order (checked at runtime under \
             test-harness/debug; a tie panics naming the site). Use when order is load-bearing: a greedy \
             loop, a take(n) budget, a shared RNG draw or counter, a clamped accumulate, a lethal pick.\n  \
           * `util::sort_value_canonical(&mut v, |x| key)` — ties are fine because tied elements are \
             INTERCHANGEABLE. Sort by the WHOLE value, not a prefix of it; then a tie means they are \
             identical. (Sorting by a prefix is exactly how the ORCA / drink-contention / boss-cull bugs \
             happened.)\n  \
           * `// SORT-OK: <reason>` above the sort — the input never comes from an ECS query (a seeded \
             generator, a constant table, pure geometry).\n\n\
         Why this is a hard gate, not a style nit: ECS query order is NOT stable across `App` instances, so \
         a sort that falls through to it makes the sim irreproducible — and a search scoring against an \
         irreproducible sim is optimizing noise. See docs/rl/2026-07-16-search-rollout-nondeterminism.md\n",
        offenders.len(),
        offenders.join("\n"),
    );
}

/// True when a `#[cfg(...)]` attribute line enables the `test` cfg — matching `#[cfg(test)]`,
/// `#[cfg(any(test, ...))]`, `#[cfg(all(test, ...))]`, and spaced variants (`#[cfg( test )]`), so a test
/// module written any of those ways is exempted (its inputs are hand-built, not ECS queries). A cheap
/// whole-token scan: `test` must appear as a complete cfg token. Stays allocation-light, matching the
/// rest of this line-scanning lint.
///
/// **String literals are stripped before tokenizing, and that is the whole point.** A feature NAME is not
/// a cfg predicate, and `-` is a token separator here, so `#[cfg(feature = "test-harness")]` tokenized raw
/// yields `test` + `harness` and matched — silently marking the rest of the file a test module and
/// skipping every sort below it. That is not hypothetical: `src/light.rs` carries that attribute at line
/// 457, which blinded this lint to the whole back half of the file (an unannotated `sort_by_key` deciding
/// a scarce per-room resource sailed through), and it does the same in every other file that gates an item
/// on the `test-harness` feature. A lint that silently stops looking is worse than no lint.
fn cfg_enables_test(attr_line: &str) -> bool {
    let mut outside_strings = String::with_capacity(attr_line.len());
    let mut in_string = false;
    for ch in attr_line.chars() {
        if ch == '"' {
            in_string = !in_string;
        } else if !in_string {
            outside_strings.push(ch);
        }
    }
    outside_strings
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|tok| tok == "test")
}

/// Pins [`cfg_enables_test`] on both sides. The false-positive half is the one that actually bit: a
/// `feature = "test-harness"` gate anywhere in a file used to switch this lint off for everything below
/// it, so the gate silently stopped gating.
#[test]
fn cfg_test_detection_ignores_feature_names() {
    // Real test modules — must still be exempted.
    assert!(cfg_enables_test("#[cfg(test)]"));
    assert!(cfg_enables_test("#[cfg( test )]"));
    assert!(cfg_enables_test("#[cfg(any(test, feature = \"x\"))]"));
    assert!(cfg_enables_test("#[cfg(all(test, unix))]"));
    // Feature names are not cfg predicates: none of these may exempt anything.
    assert!(!cfg_enables_test("#[cfg(feature = \"test-harness\")]"));
    assert!(!cfg_enables_test("#[cfg(feature = \"test_util\")]"));
    assert!(!cfg_enables_test("#[cfg(not(feature = \"test-harness\"))]"));
    assert!(!cfg_enables_test("#[cfg(feature = \"harness-test\")]"));
    // A genuine `test` predicate still counts when a feature string rides alongside it.
    assert!(cfg_enables_test("#[cfg(any(test, feature = \"test-harness\"))]"));
}
