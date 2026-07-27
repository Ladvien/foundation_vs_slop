//! Behaviour descriptors — the axes the MAP-Elites archive bins on. Split out of the former
//! single-file `coevolve.rs`; a pure move, no logic changed (FVS-N-3).

use super::*;

// ── Behaviour descriptors ────────────────────────────────────────────────────────────────────────

/// Modes that read, to a watching player, as *pressing the fight*.
fn is_squad_combat(mode: Mode) -> bool {
    // One definition, shared with the minimal criterion's agency clause.
    crate::squad_ai::surprise::is_squad_offensive(mode)
}

/// Modes that read as the swarm *committing* rather than milling or fleeing.
fn is_swarm_aggression(mode: Mode) -> bool {
    matches!(mode, Mode::Latch | Mode::Rally | Mode::Muster | Mode::Chase)
}

/// Both descriptor axes must be things a *player perceives*, because the archive's whole job is to hold
/// visibly different playstyles apart. `aggression` is the share of decisions that press the fight;
/// `exploration` is how much of the reachable map the squad actually walked.
pub fn squad_descriptor(trace: &EpisodeTrace, outcome: &EpisodeOutcome) -> BehaviorDescriptor {
    let unit_decisions: Vec<Mode> = trace
        .decisions
        .iter()
        .filter(|d| matches!(d.context.actor, crate::squad_ai::surprise::ActorKind::Role(_)))
        .map(|d| d.mode)
        .collect();
    let aggression = share(&unit_decisions, is_squad_combat);
    let exploration = if outcome.reachable_cells > 0 {
        outcome.cells_covered as f32 / outcome.reachable_cells as f32
    } else {
        0.0
    };
    BehaviorDescriptor::new(aggression, exploration)
}

/// The swarm's axes: how much it commits, and how much it holds together rather than routing. Both are
/// read straight off the decision trace, so no extra instrumentation is needed.
///
/// The second axis is **persistence** — the complement of the flee share. A swarm that scatters at the
/// first shot occupies a different niche from one that presses through fire on the ALARM pheromone. It
/// reuses `BehaviorDescriptor::exploration` as a generic second coordinate; the archive only ever needs
/// two numbers in `[0,1]`, and giving the swarm its own type would duplicate the grid for no gain.
pub fn swarm_descriptor(trace: &EpisodeTrace) -> BehaviorDescriptor {
    let creature_decisions: Vec<Mode> = trace
        .decisions
        .iter()
        .filter(|d| !matches!(d.context.actor, crate::squad_ai::surprise::ActorKind::Role(_)))
        .map(|d| d.mode)
        .collect();
    let aggression = share(&creature_decisions, is_swarm_aggression);
    let persistence = 1.0 - share(&creature_decisions, |m| m == Mode::Flee);
    BehaviorDescriptor::new(aggression, persistence)
}

/// Half-saturation constant for the world's vitality axes: the cross-species death/life count that maps to
/// the descriptor midpoint `0.5`. **Calibrated by measurement, not guessed** (`train probe` on the shipped
/// worlds at 7200 ticks: ~11–17 total deaths and ~6–18 total lives across the held-in seeds — the same
/// discipline behind `surprise::MIN_COVERAGE` and the `FearBucket` bands). At `K = 25` the shipped game sits
/// low-mid on both axes (~0.3 deaths, ~0.2–0.4 lives), so the whole deadlier/teeming corner stays as
/// headroom for the search to illuminate.
///
/// The map is a **saturating** response `x / (x + K)` (Holling Type II / Michaelis–Menten), not a
/// hard-clamped linear scale: cross-species counts span a wide range as the breeding/lethality/parasite
/// knobs move (Gras et al. 2009 — individual counts co-vary strongly with the dynamics), and a saturating
/// map keeps descriptor resolution across that whole range instead of clipping the extremes into one bin.
const VITALITY_HALF_SCALE: f32 = 25.0;

/// The **world's** axes — the two dials the search was pointed at: how many creatures DIE and how many are
/// alive (LIVES) at episode end, summed across every species the headless sim observes (squad units, crabs,
/// SCP-150 mancae, boss; mushrooms are GPU-only and shaped by the separate `train levels` search). This is
/// the "deaths and lives across all species" archive: MAP-Elites spreads worlds from graveyard to teeming
/// and a human picks the regime. Fitness stays `W·S·L` — these axes carry *diversity*, not quality.
///
/// Grounding: predator–prey turnover and biodiversity are canonical signals of a living ecosystem (Gras et
/// al., Artificial Life 15(4) 2009; Yang, arXiv:1003.5288). NOTE: the LIVES axis is total abundance (a
/// headcount sum), NOT the Shannon diversity index (which needs species proportions / evenness).
/// Each count is mapped into `[0,1)` by the saturating [`VITALITY_HALF_SCALE`] response; the
/// breeding-vs-lethality knobs (now including the parasite's brood/gestation) give the search 2-D freedom,
/// so deadly-yet-teeming and deadly-yet-depleted worlds land in different niches rather than collapsing onto
/// a diagonal.
///
/// Deliberately a FIXED half-scale, not the per-episode peak census (`EpisodeOutcome::peak_population` /
/// `crab_peak` etc.): normalising an axis by a quantity the same genome moves would fold "how populous"
/// back out of the axis and re-couple the two dials. The peak fields feed `train probe`'s calibration
/// report only — if the two ever look contradictory, this comment is the tiebreak: the descriptor is
/// absolute-by-design, the census is diagnostic.
pub fn world_descriptor(outcome: &EpisodeOutcome) -> BehaviorDescriptor {
    // Saturating (Holling Type II) map so a wide count range keeps descriptor resolution; see the constant.
    let softsat = |x: u32| x as f32 / (x as f32 + VITALITY_HALF_SCALE);
    BehaviorDescriptor::new(softsat(outcome.total_deaths()), softsat(outcome.total_lives()))
}

fn share(modes: &[Mode], pred: impl Fn(Mode) -> bool) -> f32 {
    if modes.is_empty() {
        return 0.0;
    }
    modes.iter().filter(|m| pred(**m)).count() as f32 / modes.len() as f32
}
