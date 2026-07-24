//! **Rollout evaluator for the policy (neuroevolution) population** (feature `test-harness`).
//!
//! Sibling of [`super::behavior_eval`] / [`super::audio_eval`]: decode a [`PolicyGenome`] into a
//! [`NeuralPolicy`], run two full headless rollouts on different worlds with that learned controller
//! installed at the squad policy seam ([`rollout_with_policy`]), gate each on the behavioural
//! [`minimal_criterion`], and score the pair with the same witnessed-learnable-surprise fitness
//! ([`fitness`]) as every other population. The objective is identical — emergent, *interesting* squad
//! behaviour — only the thing being evolved (the decision layer's weights) differs.

use std::sync::Arc;

use crate::ai::utility::{Mode, MODE_COUNT};
use crate::config::WorldConfig;

use super::evaluate::rollout_with_policy;
use super::fairness::{mode_concentration, survival_competence};
use super::policy::SquadPolicy;
use super::policy_genome::{self, PolicyGenome};
use super::surprise::{fitness, minimal_criterion, ActorKind, EpisodeTrace, ModePrior};

/// One policy genome's score: the fitness scalar plus the two MAP-Elites descriptor axes. For the learned
/// policy the axes are `(initiative, caretaking)` (see [`policy_descriptor`]) — chosen because the shared
/// `squad_descriptor` axes (aggression × map-coverage) are near-constant across *feasible* policies and
/// collapse the archive, whereas mode-usage genuinely varies.
pub struct PolicyEvaluation {
    pub fitness: f32,
    pub axes: (f32, f32),
}

/// Evaluate a policy genome. `None` (a rejected candidate) when it is infeasible, fails to decode, or
/// either rollout fails the minimal criterion — one path, never a degraded fallback score.
pub fn evaluate(
    genome: &PolicyGenome,
    prior: &ModePrior,
    seeds: &[u64],
    ticks: u32,
) -> Option<PolicyEvaluation> {
    policy_genome::is_feasible(genome).ok()?;
    // Decode ONCE. `NeuralPolicy` is `Clone`, so the per-rollout factory clones the decoded net rather than
    // re-decoding — no repeated parse, and no `expect`/panic inside the factory (the one-path/no-panic rule).
    let policy = policy_genome::decode(genome).ok()?;
    let seed_a = *seeds.first()?;
    let seed_b = *seeds.get(1)?;

    let pa = policy.clone();
    let a = rollout_with_policy(
        Arc::new(move || Box::new(pa.clone()) as Box<dyn SquadPolicy>),
        None,
        None,
        None,
        seed_a,
        ticks,
    );
    minimal_criterion(&a.outcome).ok()?;

    let pb = policy.clone();
    let b = rollout_with_policy(
        Arc::new(move || Box::new(pb.clone()) as Box<dyn SquadPolicy>),
        None,
        None,
        None,
        seed_b,
        ticks,
    );
    minimal_criterion(&b.outcome).ok()?;

    // Behaviour axes chosen so *feasible* policies actually spread (see `policy_descriptor`): the shared
    // aggression × map-coverage axes are near-constant for any policy that clears `minimal_criterion`, so a
    // grid over them bins every survivor into one cell. Fitness is unchanged — descriptors carry diversity.
    let axes = policy_descriptor(&a.trace);
    Some(PolicyEvaluation { fitness: fitness(&a.trace, &b.trace, prior).score(), axes })
}

/// Squad-mode usage histogram (length [`MODE_COUNT`]) over a trace's **unit** decisions — the input to
/// `fairness::mode_concentration`. Creature decisions are excluded: exploitability is about how the *squad*
/// wins.
pub fn unit_mode_histogram(trace: &EpisodeTrace) -> [u32; MODE_COUNT] {
    let mut counts = [0u32; MODE_COUNT];
    for d in &trace.decisions {
        if matches!(d.context.actor, ActorKind::Role(_)) {
            counts[d.mode.index()] += 1;
        }
    }
    counts
}

/// The **behaviour-characterisation axes for the learned squad policy**, chosen so that *feasible* neural
/// policies actually spread across them. The shared `squad_descriptor` axes — combat-share `aggression` and
/// map-coverage `exploration` — are near-constant for any policy that clears the survival
/// `minimal_criterion` (a surviving squad barely presses the fight, and coverage tracks the player anchor,
/// not the policy), so a MAP-Elites grid over them collapses every feasible policy into one cell (observed:
/// the isotropic and CMA runs both filled 1–2 cells, all at aggression≈0). MAP-Elites only illuminates the
/// *dimensions of variation you choose*, and those must be dimensions feasible solutions vary in (Mouret &
/// Clune, "Illuminating search spaces by mapping elites", arXiv:1504.04909). Feasible policies vary in *how
/// they use their behaviours*, so both axes are read off the squad mode histogram:
///
/// - **initiative** — the share of unit decisions that are *not* the default `FollowAnchor` tether: how much
///   the squad acts on its own vs. trailing the player anchor.
/// - **caretaking** — among those non-follow decisions, the fraction spent on self-directed care/defence
///   (`TendWounded`/`Ward`/`Commune`/`Regroup`/`SecureDoor`/`DeploySensor`) rather than
///   scouting/watching/fleeing: a medic-minded squad vs. a vigilant/roaming one.
///
/// Both are pure functions of integer mode counts — deterministic, order-independent, no RNG — so the
/// committed archive stays bit-reproducible. Returned as the generic `(x, y)` grid coordinate the archive
/// bins, exactly as `swarm_descriptor` reuses the two `BehaviorDescriptor` slots for its own axes.
fn policy_descriptor(trace: &EpisodeTrace) -> (f32, f32) {
    policy_axes(&unit_mode_histogram(trace))
}

/// Pure `(initiative, caretaking)` axis math for [`policy_descriptor`], split out so it is unit-testable
/// without an ECS trace. Empty history → `(0, 0)`; an all-`FollowAnchor` squad → `(0, 0.5)` (no non-follow
/// signal, so `initiative` alone pins it, and `caretaking` sits neutral rather than biasing a corner).
fn policy_axes(hist: &[u32; MODE_COUNT]) -> (f32, f32) {
    let total: u32 = hist.iter().sum();
    if total == 0 {
        return (0.0, 0.0);
    }
    let follow = hist[Mode::FollowAnchor.index()];
    let non_follow = total - follow;
    let initiative = non_follow as f32 / total as f32;

    // Self-directed care/defence modes (vs. scouting/watching/fleeing). One list, kept beside the axis doc.
    const CARE: [Mode; 6] = [
        Mode::TendWounded,
        Mode::Ward,
        Mode::Commune,
        Mode::Regroup,
        Mode::SecureDoor,
        Mode::DeploySensor,
    ];
    let care: u32 = CARE.iter().map(|&m| hist[m.index()]).sum();
    let caretaking = if non_follow > 0 { care as f32 / non_follow as f32 } else { 0.5 };
    (initiative, caretaking)
}

/// One playtester genome's exploit signal on a config: the **competence** it achieves (mean survival
/// fraction across the seeds — the search's fitness, maximise it to find the strongest play) and the
/// **strategy concentration** of that play (Herfindahl over the squad mode histogram). Together they feed
/// `fairness::exploitability`.
pub struct PlaytesterEvaluation {
    /// Mean survival fraction across the seeds — the difficulty gauge the search maximises.
    pub competence: f32,
    /// Herfindahl concentration of the squad's mode usage — `1` = one dominant tactic.
    pub concentration: f32,
}

/// Evaluate a **playtester** policy: install the learned controller, run one rollout per seed against
/// `config`, and report how well it kept the squad alive and how concentrated its play was.
///
/// Unlike [`evaluate`], the objective is *competence* (survival), not witnessed-learnable-surprise — this is
/// the agent whose job is to *beat* the config, so that its best achievable play measures difficulty and its
/// style measures exploitability. It deliberately does **not** gate on `minimal_criterion`: a config a strong
/// player trivially survives is exactly the exploit we want surfaced, not discarded. `None` only when the
/// genome is infeasible / fails to decode, or no seed was supplied — one path, no degraded fallback score.
pub fn evaluate_playtester(
    genome: &PolicyGenome,
    config: Option<WorldConfig>,
    seeds: &[u64],
    ticks: u32,
) -> Option<PlaytesterEvaluation> {
    policy_genome::is_feasible(genome).ok()?;
    let policy = policy_genome::decode(genome).ok()?;
    if seeds.is_empty() {
        return None;
    }

    let mut competence_sum = 0.0f32;
    let mut hist = [0u32; MODE_COUNT];
    for &seed in seeds {
        let p = policy.clone();
        let r = rollout_with_policy(
            Arc::new(move || Box::new(p.clone()) as Box<dyn SquadPolicy>),
            config.clone(),
            None,
            None,
            seed,
            ticks,
        );
        competence_sum += survival_competence(r.outcome.survivors, r.outcome.squad_size);
        let seed_hist = unit_mode_histogram(&r.trace);
        for (h, c) in hist.iter_mut().zip(seed_hist.iter()) {
            *h += *c;
        }
    }
    Some(PlaytesterEvaluation {
        competence: competence_sum / seeds.len() as f32,
        concentration: mode_concentration(&hist),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hist_of(pairs: &[(Mode, u32)]) -> [u32; MODE_COUNT] {
        let mut h = [0u32; MODE_COUNT];
        for &(m, c) in pairs {
            h[m.index()] = c;
        }
        h
    }

    #[test]
    fn policy_axes_separate_playstyles_the_old_axes_could_not() {
        // Empty history → origin, never a panic (no decisions logged).
        assert_eq!(policy_axes(&[0; MODE_COUNT]), (0.0, 0.0));

        // A squad that only follows the anchor: zero initiative, neutral (0.5) caretaking — pinned by axis 1.
        let (i, c) = policy_axes(&hist_of(&[(Mode::FollowAnchor, 100)]));
        assert!(i.abs() < 1e-6);
        assert!((c - 0.5).abs() < 1e-6);

        // Half-follow and every non-follow decision is care → initiative 0.5, caretaking 1.0.
        let (i, c) = policy_axes(&hist_of(&[(Mode::FollowAnchor, 50), (Mode::TendWounded, 50)]));
        assert!((i - 0.5).abs() < 1e-6 && (c - 1.0).abs() < 1e-6);

        // The point of the fix: two feasible policies the OLD aggression axis scored identically (~0 combat)
        // now land in DIFFERENT cells — a caretaker (all Ward) vs. a scout (all Examine): same initiative,
        // opposite caretaking.
        let caretaker = policy_axes(&hist_of(&[(Mode::FollowAnchor, 20), (Mode::Ward, 80)]));
        let scout = policy_axes(&hist_of(&[(Mode::FollowAnchor, 20), (Mode::Examine, 80)]));
        assert!((caretaker.0 - scout.0).abs() < 1e-6, "same initiative (0.8 each)");
        assert!(caretaker.1 > 0.9 && scout.1 < 0.1, "caretaking axis separates them");
    }
}
