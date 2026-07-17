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

        // Everything from a `#[cfg(test)]` module to EOF is test-only: its inputs are hand-built Vecs, not
        // ECS queries, so the contract does not apply.
        let test_mod = lines.iter().position(|l| l.trim_start().starts_with("#[cfg(test)]"));

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
