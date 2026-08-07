//! **FVS-K-4's third clause, as a lint** — no shipped copy may present the deprecated theming as canon.
//!
//! # What was deprecated, and why it needs a lint rather than a memory
//!
//! An early lore pass explored **semiotic decay** as the antagonist: meaning coming loose from things,
//! countered by the Gat-Hayes Semantic Stabilization Device (SCP-6254), with SCP-2521 as a related
//! object. It is good material and the research documents that developed it are kept — but the game
//! settled on a different antagonist. **SCP-9191 is a generator**, and its horror is that its output is
//! *bad*: detail lavished where it does not matter, proportions subtly wrong, an enormous amount of
//! care spent by something that does not understand what it is making. That is the uncanny valley
//! ([UV-REV], [UV-FMRI]), not semiotics.
//!
//! The two are not compatible flavours of one idea. Semiotic decay says meaning is *draining out* of
//! the world; the generator says meaning is being *mass-produced badly*. A player told both is being
//! told the antagonist is two different things, and the research economy — which cashes out as
//! restoring curation against an out-of-control generator — only makes sense under the second.
//!
//! So this is a lint, not a note in a design doc, for the reason `tests/panic_budget.rs` and
//! `tests/genome_coverage.rs` are lints: a rule people remember is a rule that comes back. The lore
//! documents are seductive and detailed, and the failure mode is somebody mining one for flavour text
//! two months from now and reintroducing a contradiction nobody notices until a player does.
//!
//! # What is scanned, and what is deliberately not
//!
//! **Scanned:** everything the player can encounter — `src/` (dialogue lines, HUD strings, log copy),
//! `assets/config/config.ron` (the authored conversations and rule text), and `README.md`.
//!
//! **Not scanned:** `docs/lore/`. Those are *research*, not copy, and deleting them would lose the work
//! that produced the decision. They carry a deprecation banner instead — pointing here — so a reader
//! meets the ruling before the material.

use std::path::Path;

mod common;

/// Terms that name the deprecated antagonist theming.
///
/// Deliberately narrow: these are proper nouns and coined phrases, not ordinary words. A broad match on
/// something like "meaning" or "decay" would fire on the mold's `growth_decay` and on half the field
/// code, and a lint that cries wolf gets deleted.
const DEPRECATED: &[&str] = &[
    "semiotic",
    "Gat-Hayes",
    "Gat Hayes",
    "GHSSD",
    "Semantic Stabilization",
    "SCP-6254",
    "SCP-2521",
];

fn offending_lines(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            let lower = l.to_lowercase();
            DEPRECATED.iter().any(|d| lower.contains(&d.to_lowercase()))
        })
        .map(|(i, l)| (i + 1, l.trim().to_string()))
        .collect()
}

/// **Lines that develop the deprecated theming, as opposed to disclaiming it.**
///
/// A line naming the material *and* calling it deprecated is doing the thing this lint wants: it is
/// pointing at the ruling. `docs/lore/2026-08-01-scp-gear.md` says the Coherence Anchor was
/// "re-themed off the deprecated Gat-Hayes stabilizer" — one sentence, whose whole content is that
/// the theming is gone. Treating that as development demanded a banner announcing a deprecation the
/// line had just announced.
///
/// Narrow on purpose, and applied **only** to the docs/lore banner check — never to
/// [`scan`], which asks a different question of shipped copy: whether the words appear at all, for
/// which "we call it deprecated" is not an excuse.
///
/// A document that genuinely develops the theming still has its other lines, so this cannot excuse
/// one: it only lets a doc that mentions it once, to disown it, avoid a banner about itself.
fn developing_lines(text: &str) -> Vec<(usize, String)> {
    offending_lines(text)
        .into_iter()
        .filter(|(_, l)| !l.to_lowercase().contains("deprecated"))
        .collect()
}

fn scan(path: &Path, hits: &mut Vec<String>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        // A file that will not read as UTF-8 is a binary asset, not copy. Skipping is correct here and
        // is not a fallback: there is nothing to check.
        return;
    };
    for (line, content) in offending_lines(&text) {
        hits.push(format!("{}:{line}: {content}", path.display()));
    }
}


#[test]
fn no_shipped_copy_presents_the_deprecated_theming_as_canon() {
    let mut hits = Vec::new();
    // Not just `src/` — see `common::source_roots` for why the extracted crates are in the walk.
    for path in common::source_roots::scanned_sources() {
        scan(&path, &mut hits);
    }
    scan(Path::new("assets/config/config.ron"), &mut hits);
    scan(Path::new("README.md"), &mut hits);

    assert!(
        hits.is_empty(),
        "FVS-K-4: shipped copy references the DEPRECATED semiotic-decay theming.\n\n{}\n\n\
         SCP-9191 is a GENERATOR whose output is the uncanny valley — detail lavished where it does \
         not matter, by something that does not understand what it is making. Semiotic decay (meaning \
         draining out of the world) is a different and incompatible antagonist, and the research \
         economy only makes sense under the generator reading. The material is kept as research in \
         docs/lore/; it is not canon. See tests/lore_canon.rs.",
        hits.join("\n")
    );
}

#[test]
fn the_lore_research_is_kept_but_marked_deprecated() {
    // The other half, and the one that keeps the rule findable. The documents are good work and stay —
    // but a reader who opens one must meet the ruling before the material, or the lint above just
    // becomes a surprise at commit time.
    let dir = Path::new("docs/lore");
    let Ok(entries) = std::fs::read_dir(dir) else {
        panic!("docs/lore/ is missing — the research that produced the decision must not be deleted");
    };
    let mut unmarked = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if developing_lines(&text).is_empty() {
            continue; // this document does not develop the deprecated theming
        }
        // The banner must be near the top, where it is read first.
        let head: String = text.lines().take(12).collect::<Vec<_>>().join("\n");
        if !head.contains("DEPRECATED") {
            unmarked.push(path.display().to_string());
        }
    }
    assert!(
        unmarked.is_empty(),
        "these lore documents develop the deprecated semiotic-decay theming but do not say so in \
         their first 12 lines:\n{}\n\nAdd a banner pointing at FVS-K-4, so the material is not mined \
         for flavour text by someone who never saw the ruling.",
        unmarked.join("\n")
    );
}

#[test]
fn the_generator_reading_is_actually_present_in_shipped_copy() {
    // The positive half. A lint that only forbids the old theming would pass just as happily on a game
    // with NO antagonist theming at all — which is the state K-4 was filed to fix, and which the
    // negative test above cannot distinguish from success.
    let config = std::fs::read_to_string("assets/config/config.ron").expect("config.ron reads");
    for needle in ["copy", "generator", "made"] {
        assert!(
            config.to_lowercase().contains(needle),
            "the authored conversations must actually voice the generator reading; \
             {needle:?} appears nowhere in config.ron"
        );
    }
}
