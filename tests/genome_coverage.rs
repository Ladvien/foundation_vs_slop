//! **FVS-I-2 — "every feature must evolve", as a lint.**
//!
//! `CLAUDE.md` says *"Ensure every feature added is correctly included in the RL/QD systems for
//! evolving."* That has been a rule people remember, and the backlog records what forgetting it costs:
//! `GoreSettings.autogib_*` drifted un-evolved and produced a 5/5-win → wipe regression, and `mold` +
//! `almond` were scored by the rollout while `WorldEliteDoc`/`apply_dim` omitted them — 23 of 102 knobs
//! the search tuned and the game could never ship.
//!
//! # What this checks, and what it deliberately does not
//!
//! Per-*knob* drift inside an already-evolved slice is **already** caught, and by something stronger
//! than a lint: `world_genome::authored_round_trips_exactly` asserts `decode ∘ encode` is the identity
//! on the shipped config, so a field added to `SimTuning` without a matching `encode`/`BOUNDS`/`decode`
//! entry fails immediately. The audio and behavior genomes carry the same guard.
//!
//! What nothing catches is the **coarser** drift: a whole config slice appearing that no genome touches
//! at all. Nobody notices, because there is no round-trip to break — the slice simply is not in one.
//! That is the gap this closes.
//!
//! # It is a ledger, not a ban
//!
//! Several slices *should* be exempt, and for good reasons that are worth stating once rather than
//! re-litigating: cosmetics cannot move the sim, and three slices are deliberately outside the search
//! because they define the **objective** rather than the difficulty — a search free to retune what
//! "winning" or "capturing" or "research" means would be moving the measuring stick, and archive
//! fitness would stop being comparable between bakes.
//!
//! So the failure mode is a *new, unclassified* slice. Adding one makes this test fail with the slice's
//! name and a demand for a decision, which is exactly the review moment the rule wants. Same shape as
//! `tests/panic_budget.rs`: a ratchet you have to consciously move, not a blanket prohibition that gets
//! switched off within a day.

use std::collections::BTreeMap;

/// Which search owns a slice — or why nothing does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coverage {
    /// Evolved by `squad_ai::world_genome` (the `WorldConfig` set).
    World,
    /// Evolved by `squad_ai::level_genome`.
    Level,
    /// Evolved by `squad_ai::audio_genome`.
    Audio,
    /// Evolved by `squad_ai::behavior_genome`.
    Behavior,
    /// Render/FX only. It cannot reach `snapshot_hash`, so there is nothing for a search to optimise.
    Cosmetic,
    /// **Deliberately** outside every search because it defines the objective, not the difficulty.
    Objective,
    /// Un-evolved and it probably should not be. Listed so the gap is visible and counted.
    Gap,
}

/// The ledger. **Every top-level slice of `config.ron` must appear here.**
fn ledger() -> BTreeMap<&'static str, (Coverage, &'static str)> {
    BTreeMap::from([
        ("dungeon", (Coverage::Level, "layout/room dials — `level_genome`")),
        ("placement", (Coverage::Level, "furniture density + Metropolis weights — `level_genome`")),
        ("ai_tuning", (Coverage::World, "field propagation — `WorldConfig::ai`")),
        ("behavior", (Coverage::Behavior, "utility-brain weights — `behavior_genome`")),
        ("sim", (Coverage::World, "simulation dynamics — `WorldConfig::sim`")),
        ("mycelia", (Coverage::Level, "mushroom amount — `level_genome`")),
        ("lighting", (Coverage::World, "gameplay illuminance — `WorldConfig::lighting`")),
        ("almond_water", (Coverage::World, "belief/inversion water — `WorldConfig::almond`")),
        ("mold", (Coverage::World, "reaction-diffusion mold — `WorldConfig::mold`")),
        ("audio", (Coverage::Audio, "acoustic stimulus/salience — `audio_genome`")),
        (
            "containment",
            (
                Coverage::Objective,
                "the RULES: what capturing an anomaly MEANS. A search free to retune a capture basin \
                 would be solving a different game each generation. The LOGISTICS (how many devices, \
                 how far each verb reaches) are difficulty and DO evolve — they live in `sim.containment`.",
            ),
        ),
        (
            "session",
            (
                Coverage::Objective,
                "the win condition. A search allowed to shorten the timer would 'solve' every level by \
                 making it shorter.",
            ),
        ),
        (
            "research",
            (
                Coverage::Objective,
                "the Thaumiel curriculum: what research is WORTH. Same argument as `containment`.",
            ),
        ),
        (
            "gore",
            (
                Coverage::Gap,
                "FVS-I-2's headline example. `autogib_*` is un-evolved and it has already cost a \
                 5/5-win -> wipe regression, because chunk counts reach `crab::assign_meat_targets` \
                 and therefore the sim. This is a real gap, not an exemption.",
            ),
        ),
        ("hair", (Coverage::Cosmetic, "strand rendering only")),
        ("impact_fx", (Coverage::Cosmetic, "hit sparks/decals only")),
        ("vhs", (Coverage::Cosmetic, "full-screen post-process only")),
        ("dialogue", (Coverage::Cosmetic, "authored conversation content, not a tunable")),
    ])
}

/// Read the top-level slice names out of the shipped config.
///
/// Parsed as a generic `ron::Value` rather than as `GameConfig`, deliberately: deserialising into the
/// struct would only ever return the fields the struct already knows about, which is precisely the
/// blind spot this test exists to cover. The point is to see the FILE.
fn config_slices() -> Vec<String> {
    let text = std::fs::read_to_string("assets/config/config.ron")
        .expect("assets/config/config.ron must be readable from the project root");
    let value: ron::Value = ron::from_str(&text).expect("config.ron must parse as RON");
    match value {
        ron::Value::Map(map) => map
            .iter()
            .filter_map(|(k, _)| match k {
                ron::Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        other => panic!("config.ron's top level should be a map, got {other:?}"),
    }
}

#[test]
fn every_config_slice_is_classified_as_evolved_exempt_or_a_known_gap() {
    let ledger = ledger();
    let unclassified: Vec<String> =
        config_slices().into_iter().filter(|s| !ledger.contains_key(s.as_str())).collect();
    assert!(
        unclassified.is_empty(),
        "config.ron has slice(s) no genome covers and no exemption names: {unclassified:?}\n\n\
         CLAUDE.md: \"Ensure every feature added is correctly included in the RL/QD systems for \
         evolving.\" Adding a slice is the moment to decide which of the four searches owns it — or to \
         record WHY it is exempt (cosmetic, or an objective the search may not move). Add it to \
         `ledger()` in this file with a reason. This failing is the review, not an obstacle to it."
    );
}

#[test]
fn the_ledger_does_not_describe_slices_that_no_longer_exist() {
    // The other direction: a slice deleted from the config should not leave a stale justification
    // behind, or the ledger slowly becomes a list of claims about a file that has moved on.
    let slices = config_slices();
    let stale: Vec<&str> =
        ledger().keys().copied().filter(|k| !slices.iter().any(|s| s == k)).collect();
    assert!(stale.is_empty(), "ledger() names slices config.ron no longer has: {stale:?}");
}

#[test]
fn the_known_gaps_are_counted_so_they_cannot_quietly_grow() {
    // A ratchet, exactly like `tests/panic_budget.rs`. Un-evolved gameplay knobs are a real debt; the
    // useful property is not "there are none" (there is one, and closing it is its own work) but that
    // adding another is a deliberate, reviewable act rather than an accident.
    const KNOWN_GAPS: usize = 1;
    let gaps: Vec<&str> = ledger()
        .iter()
        .filter(|(_, (c, _))| *c == Coverage::Gap)
        .map(|(k, _)| *k)
        .collect();
    assert_eq!(
        gaps.len(),
        KNOWN_GAPS,
        "un-evolved gameplay slices changed: {gaps:?}. If you CLOSED one, lower KNOWN_GAPS — that is \
         the ratchet tightening. If you added one, it needs the justification a `Gap` entry does not \
         have."
    );
}

#[test]
fn no_slice_is_classified_as_both_evolved_and_exempt() {
    // A slice cannot be owned by a search AND be outside every search. This is cheap and it catches a
    // copy-paste while editing the ledger, which is the realistic way it would go wrong.
    for (slice, (coverage, why)) in ledger() {
        assert!(!why.trim().is_empty(), "{slice} has no stated reason");
        let evolved = matches!(
            coverage,
            Coverage::World | Coverage::Level | Coverage::Audio | Coverage::Behavior
        );
        let exempt = matches!(coverage, Coverage::Cosmetic | Coverage::Objective);
        assert!(
            evolved ^ exempt || coverage == Coverage::Gap,
            "{slice} is classified as both evolved and exempt"
        );
    }
}
