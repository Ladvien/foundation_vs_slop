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
//! Per-*knob* drift inside an already-**encoded struct** is caught by something stronger than a lint:
//! `world_genome::authored_round_trips_exactly` asserts `decode ∘ encode` is the identity on the
//! shipped config, so a field added to `SimTuning` without a matching `encode`/`BOUNDS`/`decode` entry
//! fails immediately. The audio and behavior genomes carry the same guard.
//!
//! ⚠️ **"Encoded struct" is narrower than "evolved slice", and the difference is where this test used
//! to overstate itself.** A slice can be owned by a search while a whole sub-struct inside it is in no
//! genome at all — and then there is no round-trip to break either, because nothing round-trips it.
//! `placement` is classified `Level` and its 15 `MetropolisWeights` are encoded nowhere; `behavior` is
//! classified `Behavior` and 11 of 13 `PerceptionTuning` knobs are likewise absent. Both are real and
//! tracked, and both were invisible here while the headline said "0 gaps". See [`partials`].
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
    /// **Owned by a search, but a named sub-struct inside it is not encoded.**
    ///
    /// This variant exists because the ledger was **slice-deep, and its headline read stronger than
    /// the truth**: every slice was classified, `KNOWN_GAPS` was 0, and "0 gaps" therefore reported
    /// full coverage while two whole sub-structs — 15 Metropolis weights and 11 of 13 perception
    /// knobs — were unevolved and tracked in the backlog. Nothing here was wrong; it was the
    /// *granularity* that made a true statement misleading. Found by the 2026-07-31 codebase review.
    ///
    /// The honest invariant is not "no gaps" but **"no UNCLASSIFIED gaps, and N known partials"**.
    Partial,
}

/// One partially-covered slice: `(evolved, total, what is missing, tracking item)`.
///
/// ⚠️ **These counts are hand-recorded, not machine-derived, and that is a deliberate limit.** The
/// obvious mechanical version — count leaf knobs in `config.ron` per slice and assert the number —
/// was considered and rejected: `placement.furniture` is an asset *catalogue* whose leaf count moves
/// every time an artist adds a prop, so the test would fail on art changes that have nothing to do
/// with evolution. A test that cries wolf on unrelated work gets suppressed, which is worse than the
/// overstatement it replaced. Struct field counts are also not derivable without reflection
/// (`GoreSettings` has 6 top-level fields and ~30 leaf knobs through nesting), so there is no cheap
/// honest automation here.
///
/// What this DOES buy: the numbers and their tracking items are stated where the coverage claim is
/// made, so the headline can no longer read as completeness, and
/// [`the_known_partials_are_counted_so_they_cannot_quietly_grow`] ratchets the count.
struct Partial {
    slice: &'static str,
    evolved: usize,
    total: usize,
    missing: &'static str,
    tracked_as: &'static str,
}

/// The partial-coverage register. Counted 2026-07-31 against the structs named in `missing`.
fn partials() -> Vec<Partial> {
    vec![
        Partial {
            slice: "placement",
            evolved: 0,
            total: 15,
            missing: "`placement::solvers::metropolis::MetropolisWeights` — every knob, none encoded",
            tracked_as: "FVS-I-8 (which FAILED the FVS-I-6 descriptor audit: the level archive bins on \
                         clutter x infestation, and all 15 knobs tune ARRANGEMENT, never counts — so \
                         encoding them as-is would be FVS-N-21 at 15x scale)",
        },
        Partial {
            slice: "behavior",
            evolved: 2,
            total: 13,
            missing: "`behavior_tuning::PerceptionTuning` — only `leash` and `squad_think_interval` \
                      are encoded; the 11 Schmitt-band sight knobs are not",
            tracked_as: "FVS-I-9 (blocked on the same descriptor question: these are SQUAD knobs and \
                         the behaviour archive bins on the SWARM's aggression x persistence)",
        },
    ]
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
                Coverage::World,
                "FVS-I-7, landed 2026-07-31: the 8 dials with a causal path to the world archive's \
                 `deaths` axis are `gore::GoreDynamics` in the world genome (`max_gibs`, \
                 `chunk_restitution`, `gib_friction`, the four `autogib_*`, `meat_count`). The ~22 \
                 cosmetic knobs are deliberately NOT encoded, scoped by the FVS-I-6 audit -- a gene \
                 no descriptor can see makes the archive worse (FVS-N-21), so `spray_*`, `pool_*`, \
                 `droplet_*`, `dry_time`, `meat_size` and the colours stay authored. NB three of \
                 those four `autogib_*` genes now WRITE to the `fracture` slice below -- the gene \
                 group is still one struct, its storage is split by role.",
            ),
        ),
        (
            "fracture",
            (
                Coverage::World,
                "how the character mesh is CUT (`bevy_carnage::FractureSettings`), split out of the \
                 `gore` slice when the fracture became its own crate. **The coverage did not change \
                 with the move**: `pieces_base`, `min_pieces` and `max_pieces` are the same three \
                 `gore::GoreDynamics` genes the world genome has encoded since FVS-I-7, and they \
                 still reach here through `GoreDynamics::apply_to`. `ref_extent` and `min_fraction` \
                 stay authored for the same reason they always were -- `ref_extent` is the reference \
                 scale `pieces_base` is expressed against, so evolving both would be one gene \
                 fighting itself, and `min_fraction` is a floor on chunk size that the `max_pieces` \
                 clamp already bounds from the other end.",
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
fn the_known_partials_are_counted_so_they_cannot_quietly_grow() {
    // The companion ratchet to `the_known_gaps_are_counted…`, and the reason that test's `0` no longer
    // reads as "everything is evolved". A slice can be owned by a search and still have a whole
    // sub-struct outside it; before FVS-C5 that was invisible here and visible only in the backlog.
    //
    // Raise this ONLY with a `Partial` entry that names what is missing and what tracks it. Lower it
    // when a search actually encodes the sub-struct — that is the ratchet tightening.
    const KNOWN_PARTIALS: usize = 2;
    let p = partials();
    let names: Vec<&str> = p.iter().map(|x| x.slice).collect();
    assert_eq!(
        p.len(),
        KNOWN_PARTIALS,
        "partially-evolved slices changed: {names:?}. Closing one? Lower KNOWN_PARTIALS. Adding one? \
         It needs a `Partial` entry naming the sub-struct and its tracking item."
    );

    for x in &p {
        assert!(
            x.evolved < x.total,
            "{} is listed as partial but {} of {} knobs are evolved — if coverage is complete, delete \
             the entry and lower KNOWN_PARTIALS",
            x.slice,
            x.evolved,
            x.total
        );
        assert!(!x.missing.trim().is_empty(), "{} must name what is missing", x.slice);
        assert!(!x.tracked_as.trim().is_empty(), "{} must name its tracking item", x.slice);
        // A partial must be a slice the ledger says a search OWNS. A partial on a cosmetic or
        // objective slice is a contradiction: nothing is supposed to evolve there at all.
        let (coverage, _) = ledger()[x.slice];
        assert!(
            matches!(
                coverage,
                Coverage::World | Coverage::Level | Coverage::Audio | Coverage::Behavior
            ),
            "{} is listed as partially evolved but the ledger classifies it as {coverage:?}",
            x.slice
        );
    }
}

#[test]
fn the_known_gaps_are_counted_so_they_cannot_quietly_grow() {
    // A ratchet, exactly like `tests/panic_budget.rs`. Un-evolved gameplay knobs are a real debt; the
    // useful property is not "there are none" (there is one, and closing it is its own work) but that
    // adding another is a deliberate, reviewable act rather than an accident.
    // Was 1 (`gore`) until FVS-I-7 encoded the 8 sim-relevant gore dials on 2026-07-31 — the ratchet
    // tightening, which is the direction this test exists to allow.
    //
    // ⚠️ **`0` here means "no slice is wholly un-owned". It does NOT mean everything is evolved.**
    // Coverage is slice-deep, so a search can own a slice while a whole sub-struct inside it goes
    // unencoded — which is true of two slices today. See [`partials`] and
    // `the_known_partials_are_counted_so_they_cannot_quietly_grow` for the knob-level picture; this
    // number alone overstated coverage until that register was added (2026-07-31 review, C5).
    const KNOWN_GAPS: usize = 0;
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
