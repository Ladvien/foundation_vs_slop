//! **Source lint: the panic budget only ratchets down.**
//!
//! GPU-free, no `App` — runs in the `cargo test` hard gate, so it blocks on every push.
//!
//! # Why a budget rather than a ban
//!
//! `CLAUDE.md` says: *"Do not use unwrap() or anything that'd lead to a panic. Code safe. Handle
//! errors."* The obvious enforcement is `#![deny(clippy::unwrap_used)]` — and it is unusable here,
//! because the codebase predates the rule and carries **hundreds** of standing occurrences. A blanket
//! deny fails on line one, so it would be switched off within a day, and a lint that is switched off is
//! worse than no lint: it reads as enforcement while enforcing nothing. (This is why `.github/workflows/
//! ci.yml` runs clippy `continue-on-error` today.)
//!
//! So this pins the **count** instead. New code cannot add a panic site without either handling the
//! error or deliberately raising a committed number in this file — which is a reviewable act, and the
//! whole point. Removing panic sites lowers the number, and the test tells you to re-pin it downward, so
//! the budget can only shrink.
//!
//! # What is exempt, and why
//!
//! * **Test code** — a test asserting via `unwrap` is expressing an expectation, not shipping a crash.
//!   Both whole test files (`tests.rs`, `*_tests.rs`) and inline `#[cfg(test)]` modules are skipped.
//! * **`sim_harness.rs`** — the harness is test infrastructure; it is `#[cfg(feature = "test-harness")]`
//!   and never reaches a shipped binary. It is also *deliberately* panicky: several of its asserts exist
//!   to fail loudly when a determinism precondition is violated.
//! * **`bin/train.rs`** — an offline developer tool, not the game.
//!
//! Everything else — the whole simulation — is in the budget.

use std::path::{Path, PathBuf};

/// Files whose panics are not shipped game code. See the module docs.
const EXEMPT: &[&str] = &["src/sim_harness.rs", "src/bin/train.rs"];

/// **The committed budget.** Lower it when you remove panic sites; raising it is a reviewable decision,
/// not a formality — prefer handling the error.
///
/// Measured 2026-07-26 against the tree at the end of Push 2. Note how much smaller this is than a raw
/// `rg -c '.unwrap()'` over `src/` suggests (~248): almost all of that is test code and prose in doc
/// comments. The shipped simulation carries a couple of dozen, which is why a ratchet is realistic here.
///
/// **26 → 27 (2026-07-26, FVS-C-2):** `containment::ContainmentPlugin` validates the authored
/// `containment:` config slice at build and panics on a malformed rule. That is the established
/// one-path-no-fallback pattern for this config surface — `DungeonPlugin` does exactly the same for the
/// `dungeon:` slice — and a containment rule that cannot be parsed must stop the game at the door rather
/// than produce an anomaly that captures itself or can never be caught. Raising the budget here is the
/// reviewable act this lint exists to force; the alternative (silently defaulting a broken rule) is the
/// bug class `config::GameConfig`'s docs are written against.
const BUDGET: usize = 27;

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

/// Is this a test-only file? The module splits put most test code in its own file, so this catches the
/// bulk of it without any parsing.
fn is_test_file(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        n == "tests.rs" || n.ends_with("_tests.rs")
    })
}

/// Strip inline `#[cfg(test)]` modules by brace-matching.
///
/// Deliberately not "everything from `#[cfg(test)]` to EOF": `src/mycelia/mod.rs` has real code *after*
/// its test module, and assuming otherwise silently swallowed 77 lines during the module split. Match
/// the braces.
fn strip_test_modules(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("#[cfg(test)]") {
            // Walk to the end of the item this attribute guards.
            let mut depth = 0i32;
            let mut seen_brace = false;
            let mut j = i;
            while j < lines.len() {
                depth += lines[j].matches('{').count() as i32;
                depth -= lines[j].matches('}').count() as i32;
                if lines[j].contains('{') {
                    seen_brace = true;
                }
                if seen_brace && depth <= 0 {
                    break;
                }
                // A `#[cfg(test)] mod tests;` declaration has no body — one line.
                if !seen_brace && lines[j].trim_end().ends_with(';') && j > i {
                    break;
                }
                j += 1;
            }
            i = j + 1;
            continue;
        }
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n")
}

/// Count panic sites on a line, ignoring anything inside a `//` comment (docs quote `unwrap` constantly).
fn count_panics(line: &str) -> usize {
    let code = match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    };
    ["\
.unwrap()", ".expect(", "panic!(", "unreachable!(", "todo!(", "unimplemented!("]
        .iter()
        .map(|pat| code.matches(pat).count())
        .sum()
}

#[test]
fn the_panic_budget_does_not_grow() {
    let mut files = Vec::new();
    rust_files(Path::new("src"), &mut files);
    files.sort();
    assert!(!files.is_empty(), "found no sources under src/ — is the working dir the crate root?");

    let mut total = 0usize;
    let mut worst: Vec<(usize, String)> = Vec::new();
    for path in &files {
        let rel = path.to_string_lossy().replace('\\', "/");
        if EXEMPT.contains(&rel.as_str()) || is_test_file(path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let n: usize = strip_test_modules(&text).lines().map(count_panics).sum();
        if n > 0 {
            worst.push((n, rel));
        }
        total += n;
    }
    worst.sort_by(|a, b| b.0.cmp(&a.0));

    if total > BUDGET {
        let top: Vec<String> = worst.iter().take(8).map(|(n, f)| format!("{n:>4}  {f}")).collect();
        panic!(
            "panic budget exceeded: {total} sites, budget {BUDGET} (+{}).\n\
             Handle the error instead of adding a panic site. If the new panic is genuinely the right \
             behaviour — a loud failure at a precondition the sim cannot continue past — raise BUDGET in \
             tests/panic_budget.rs and say why in the commit.\n\
             Worst files:\n{}",
            total - BUDGET,
            top.join("\n")
        );
    }

    assert!(
        total >= BUDGET.saturating_sub(5),
        "panic budget is stale: {total} sites against a budget of {BUDGET}. Panic sites were removed — \
         lower BUDGET in tests/panic_budget.rs to {total} so the ratchet keeps its grip."
    );
}

#[cfg(test)]
mod self_tests {
    use super::*;

    #[test]
    fn comments_and_test_modules_do_not_count() {
        assert_eq!(count_panics("let x = y.unwrap();"), 1);
        assert_eq!(count_panics("// mentions .unwrap() in prose"), 0);
        assert_eq!(count_panics("let x = 1; // .expect( in a trailing comment"), 0);
        assert_eq!(count_panics("panic!(\"boom\"); unreachable!()"), 2);

        // An inline test module is stripped...
        let src = "fn a() { ok() }\n#[cfg(test)]\nmod t {\n    fn x() { y.unwrap(); }\n}\nfn b() { z.unwrap(); }";
        let stripped = strip_test_modules(src);
        assert!(!stripped.contains("fn x()"), "the test module must be stripped");
        // ...and real code AFTER it survives. This is the `mycelia/mod.rs` case that a naive
        // "strip to EOF" got wrong.
        assert!(stripped.contains("fn b()"), "code after a test module must survive");

        // A bodiless `#[cfg(test)] mod tests;` declaration is also handled.
        let decl = "fn a() {}\n#[cfg(test)]\nmod tests;\nfn b() { z.unwrap(); }";
        assert!(strip_test_modules(decl).contains("fn b()"));
    }
}
