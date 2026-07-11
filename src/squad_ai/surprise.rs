//! **Witnessed learnable-surprise** — the objective the offline behaviour search maximises.
//!
//! The goal is a player who watches the squad and the swarm and thinks *"I've never seen that."* Three
//! literatures say what that cannot mean, and together they say what it must.
//!
//! 1. **It cannot mean "harder".** Hunicke & Chapman ("The Case for Dynamic Difficulty Adjustment in
//!    Games", DOI 10.1145/1178477.1178573) found players feel *cheated* by adaptation, and that DDA works
//!    precisely because **change blindness** makes it imperceptible. An adaptation the player cannot
//!    perceive cannot astonish them. We therefore *invert* their constraint: behaviour scores **only when
//!    its cause was visible** ([`witnessed_fraction`]).
//!
//! 2. **It cannot mean "unpredictable".** Pathak et al. ("Curiosity-Driven Exploration by Self-Supervised
//!    Prediction", DOI 10.1109/cvprw.2017.70) name the **noisy-TV problem**: raw prediction error is
//!    maximised by a coin flip. Schmidhuber ("Driven by Compression Progress", arXiv:0812.4360) gives the
//!    fix — reward *compression progress*, surprise that becomes learnable. We transpose it from the
//!    learner to the **player**: a mode-transition model fitted on one rollout must predict a second one
//!    better than that rollout's own marginal ([`learnability`]).
//!
//! 3. **It must be measured against expectation.** The frozen, shipped dual-utility brain is the model the
//!    player has been trained on by every prior encounter. Surprise is the KL divergence of a candidate's
//!    realised mode distribution from that baseline **prior**, in matched perceptual contexts — Baldi &
//!    Itti's *Bayesian surprise* (KL of posterior from prior), with the player as the subject
//!    ([`bayesian_surprise`]).
//!
//! `fitness = W · S · L`, all in `[0,1]`, and **zero if any factor is zero**. Unseen behaviour is worth
//! nothing; unlearnable behaviour is worth nothing; unsurprising behaviour is worth nothing.
//!
//! This module is the *quality* axis. The *diversity* axis is the MAP-Elites archive grid
//! (`squad_ai::qd`), so fitness deliberately encodes no diversity term (Mouret & Clune, arXiv:1504.04909).
//!
//! It is also, deliberately, **not sufficient on its own**. [`minimal_criterion`] asks whether a real
//! encounter happened at all — the "neither too easy nor too hard" admission rule of Wang et al. (POET,
//! arXiv:1901.01753 §3) and of minimal-criterion coevolution.
//!
//! Which degenerate each half catches is worth stating precisely, because the obvious guess is wrong:
//!
//! - A brain that **always** picks one mode is *not* caught by the criterion. It is caught by `L`, and
//!   scores exactly `L = 0`: for a constant sequence the fitted transition row and the marginal are the
//!   *same* estimator, so no nats are saved. (This is also what Schmidhuber's principle demands — a fully
//!   predictable stream yields no compression *progress*.) Pinned by `a_constant_brain_is_not_learnable`.
//! - The degenerate `L` cannot see is a **low-structure but non-constant coward** — Flee / Regroup /
//!   FollowAnchor cycling — which has `L > 0`, `S > 0`, and `W ≈ 1`. That one is the criterion's job, and
//!   it needs an explicit *agency* clause, because the squad's rifles fire on their own
//!   (`laser::fire_laser`: "no key to hold") and the evaluation's synthetic player walks the squad into
//!   the swarm regardless of what the brain decides. See [`minimal_criterion`].

use serde::{Deserialize, Serialize};

use crate::ai::utility::{Mode, MODE_COUNT};
use crate::squad_ai::role::RoleId;

/// Additive smoothing on every estimated distribution. Without it a mode the baseline brain never chose
/// in a context has probability zero, and any candidate that chooses it scores infinite surprise — the
/// search would chase a divide-by-zero rather than a behaviour.
///
/// `0.5` is the **Krichevsky–Trofimov / Jeffreys** value rather than Laplace's `1.0`, for two *different*
/// reasons in the two places it is used — the same constant, not the same argument:
///
/// - In [`MarkovModel`] and [`marginal_log_likelihood`], `L` is a genuine *sequential coding* problem
///   (how many nats does a model of rollout A save when coding rollout B — Schmidhuber's compression
///   progress), and add-½ is the minimax-optimal KT estimator for exactly that. This is the KT argument,
///   and it applies only here.
/// - In [`ModePrior::prob`] it is a plug-in divergence estimate, not a code length. KT has no minimax
///   warrant there. Add-½ is chosen instead because it is the Jeffreys reference prior for a multinomial
///   and because Laplace's `1.0` injects `MODE_COUNT = 24` pseudo-counts, which over a few hundred real
///   decisions drags every distribution toward uniform: it caps a *deterministic* transition at
///   `(n+1)/(n+24)` and makes the baseline look surprising to itself. Both effects are measured by
///   `smoothing_bias_shrinks_as_the_prior_gains_evidence`.
///
/// Only the **prior** `q` is smoothed. The candidate's `p` is the raw empirical frequency, and terms with
/// `p = 0` drop out (`0*ln 0 = 0`).
///
/// Smoothing `p` symmetrically is the textbook answer to the plug-in KL's upward bias, and it is **wrong
/// here**, measurably: with `MODE_COUNT = 24`, add-half injects 12 pseudo-counts, which swamps any context
/// holding a small sample and drags `p` toward uniform. Against a peaked prior that *inflates* rather than
/// deflates the estimate — a self-surprise that should read ~0 measured **2.81 nats** at `n = 4`. The bias
/// direction inverts.
///
/// The plug-in bias is real (KL is convex in `p`; a singleton context contributes `-ln q(m*)`, carrying up
/// to `ln 24 ~= 3.18` nats of pure sampling artifact) and it does **not** cancel between candidates: it
/// tracks each one's context-occupancy histogram, so it would reward brains that scatter a few decisions
/// across many thinly-visited contexts — the noisy-TV failure sneaking past `L`'s guard. The remedy is
/// [`MIN_CONTEXT_SAMPLES`]: refuse to estimate a 24-way distribution from a handful of draws at all,
/// rather than pretend a prior can repair it.
///
/// Self-surprise is therefore bounded below by the prior's own smoothing floor, not by zero. That floor is
/// common to every candidate in a context, so it does not reorder them.
const ALPHA: f64 = 0.5;

/// A context contributes to `S` only once the candidate has decided there at least this many times.
///
/// A distribution over 24 modes cannot be estimated from three samples. Contexts below the floor are
/// dropped and the weights renormalised over what remains. This is what closes the context-scattering
/// exploit: a brain that touches many contexts once each now scores *nothing* for them, instead of
/// harvesting up to `ln 24` nats of estimator bias from each.
///
/// Chosen against the data, not in the abstract: a 60 s episode records ~1k unit decisions and ~20k
/// creature decisions over 192 contexts, so a context a brain genuinely inhabits clears 8 easily, while
/// one it merely passes through does not.
const MIN_CONTEXT_SAMPLES: u32 = 8;

/// Who is deciding. Eight actors: the five squad roles plus the three creature brains. Kept dense so a
/// [`Context`] packs into a small integer key.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Serialize)]
pub enum ActorKind {
    Role(RoleId),
    Crab,
    Scout,
    Smiley,
}

/// Number of distinct actors.
pub const ACTOR_COUNT: usize = RoleId::ALL.len() + 3;

impl ActorKind {
    fn index(self) -> usize {
        match self {
            ActorKind::Role(role) => {
                // `RoleId::ALL` is the canonical order; a role missing from it is a developer error the
                // `role_indices_are_dense` test catches, so `position` cannot be `None` in practice.
                RoleId::ALL.iter().position(|r| *r == role).unwrap_or(0)
            }
            ActorKind::Crab => RoleId::ALL.len(),
            ActorKind::Scout => RoleId::ALL.len() + 1,
            ActorKind::Smiley => RoleId::ALL.len() + 2,
        }
    }
}

/// Fear, coarsened to three bands. The exact scalar is far too fine to condition a distribution on — the
/// player does not perceive `fear = 0.41` versus `0.43`, they perceive *calm / wary / panicking*.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FearBucket {
    Calm,
    Wary,
    Panicked,
}

impl FearBucket {
    /// Bands at 1/3 and 2/3. `NaN` reads as `Calm`: a non-finite drive is a bug elsewhere, and this
    /// module must classify it rather than propagate it into a probability.
    pub fn of(fear: f32) -> Self {
        if fear >= 2.0 / 3.0 {
            FearBucket::Panicked
        } else if fear >= 1.0 / 3.0 {
            FearBucket::Wary
        } else {
            FearBucket::Calm
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The perceptual situation a decision was taken in — the conditioning variable of every distribution
/// here. Deliberately tiny (`ACTOR_COUNT × 3 × 2³ = 192`): a context the player can *narrate*
/// ("the Psionic, panicking, with a threat in view") rather than a 20-dimensional observation vector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Context {
    pub actor: ActorKind,
    pub fear: FearBucket,
    pub threat_known: bool,
    pub ally_down: bool,
    pub past_leash: bool,
}

/// Number of distinct contexts.
pub const CONTEXT_COUNT: usize = ACTOR_COUNT * 3 * 8;

impl Context {
    /// Dense key in `0..CONTEXT_COUNT`.
    pub fn key(&self) -> usize {
        let flags = usize::from(self.threat_known)
            | (usize::from(self.ally_down) << 1)
            | (usize::from(self.past_leash) << 2);
        (self.actor.index() * 3 + self.fear.index()) * 8 + flags
    }
}

/// One recorded decision. `witnessed` is true when the *cause* of the decision was perceptible to the
/// player — see `squad_ai::trace`, which resolves it against the squad's live line of sight.
#[derive(Clone, Copy, Debug)]
pub struct Decision {
    /// Stable per-actor id (`Entity::to_bits()`), so consecutive decisions can be threaded into
    /// per-actor sequences. Unique for the life of the entity, so a despawn cannot alias.
    pub actor_id: u64,
    pub context: Context,
    pub mode: Mode,
    pub witnessed: bool,
}

/// Every decision taken during one headless episode, in tick order.
#[derive(Clone, Debug, Default)]
pub struct EpisodeTrace {
    pub decisions: Vec<Decision>,
}

impl EpisodeTrace {
    pub fn is_empty(&self) -> bool {
        self.decisions.is_empty()
    }

    /// Per-actor decision sequences, in tick order — the input to the mode-transition model. Actors are
    /// visited in first-appearance order so the result is a pure function of the trace.
    ///
    /// Indexed by hash, not by a linear scan: a rollout carries tens of thousands of decisions across
    /// hundreds of crabs, and `fitness` walks this three times.
    fn sequences(&self) -> Vec<Vec<Mode>> {
        let mut index: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
        let mut runs: Vec<Vec<Mode>> = Vec::new();
        for d in &self.decisions {
            match index.get(&d.actor_id) {
                Some(&i) => runs[i].push(d.mode),
                None => {
                    index.insert(d.actor_id, runs.len());
                    runs.push(vec![d.mode]);
                }
            }
        }
        runs
    }
}

// ── W: witness ───────────────────────────────────────────────────────────────────────────────────

/// Modes in which the squad's brain presses the fight. Used for the `aggression` descriptor axis.
pub fn is_squad_offensive(mode: Mode) -> bool {
    matches!(mode, Mode::Overwatch | Mode::Engage | Mode::Suppress)
}

/// Modes in which a unit is doing **role work** rather than merely being carried along by the group.
///
/// The complement — `Flee`, `Regroup`, `FollowAnchor`, `Wander` — is the repertoire tail every role
/// shares: survival, cohesion, formation drift, and the safety default. A unit that only ever picks from
/// the tail has expressed no role.
///
/// This is the only evidence of squad **agency** available to [`minimal_criterion`], because no outcome
/// counter carries it: `laser::fire_laser` shoots any visible enemy regardless of the decided mode ("no
/// key to hold"), `crabs_killed` counts every crab despawn — including the Smiley's cull and crabs that
/// despawn on delivering meat home — and the evaluation's synthetic player walks the squad into the swarm
/// whatever the brain wants.
///
/// It is deliberately *duty*, not *offence*. Measured over the shipped brains on a 60 s episode, the squad
/// picks `FollowAnchor` ~92% of the time and an offensive mode 0–8 times; its rifles fire automatically,
/// so choosing to shoot is not how this game expresses squad intent. Duty is: `SecureDoor` 31–85,
/// `Examine` 30–65, `Commune` 8, `Overwatch` 8. An offence-only clause rejected the shipped game outright
/// on one of two worlds.
pub fn is_squad_duty(mode: Mode) -> bool {
    !matches!(mode, Mode::Flee | Mode::Regroup | Mode::FollowAnchor | Mode::Wander)
}

/// Fraction of decisions whose cause the player could see. **This is the inverted Hunicke constraint.**
/// An empty trace is `0.0`: nothing happened, so nothing was witnessed.
pub fn witnessed_fraction(trace: &EpisodeTrace) -> f32 {
    if trace.decisions.is_empty() {
        return 0.0;
    }
    let seen = trace.decisions.iter().filter(|d| d.witnessed).count();
    seen as f32 / trace.decisions.len() as f32
}

// ── S: Bayesian surprise ─────────────────────────────────────────────────────────────────────────

/// A conditional distribution over modes, `P(mode | context)`, stored as counts.
///
/// Built once by sweeping the **shipped** brain and committed as `assets/config/baseline_prior.ron`.
/// This is the modelling choice, not merely the cheap one: the player's expectation is formed by the
/// game as shipped, not by a per-episode control run.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModePrior {
    /// `counts[context_key][mode_index]`. Length is always [`CONTEXT_COUNT`].
    counts: Vec<[u32; MODE_COUNT]>,
}

impl Default for ModePrior {
    fn default() -> Self {
        ModePrior { counts: vec![[0; MODE_COUNT]; CONTEXT_COUNT] }
    }
}

impl ModePrior {
    /// Accumulate a trace's **witnessed** decisions.
    ///
    /// Witnessed-only, to match [`bayesian_surprise`], which conditions the candidate's `p` on the same
    /// event. Accumulating *all* baseline decisions would compute `KL(P(mode | witnessed) ‖ Q(mode | any))`
    /// — two distributions over different conditioning events — and, because the witness rate is
    /// mode-dependent (`Flee` is the one unit mode that can go unwitnessed), even a candidate *identical*
    /// to the baseline would score a spurious positive self-surprise concentrated on `Flee`. It is also
    /// what the "player as subject" framing demands: the player never saw the unwitnessed baseline
    /// decisions either, so they cannot be part of the expectation.
    pub fn observe(&mut self, trace: &EpisodeTrace) {
        for d in trace.decisions.iter().filter(|d| d.witnessed) {
            self.counts[d.context.key()][d.mode.index()] += 1;
        }
    }

    pub fn from_traces(traces: &[EpisodeTrace]) -> Self {
        let mut prior = ModePrior::default();
        for t in traces {
            prior.observe(t);
        }
        prior
    }

    /// Reject a prior whose shape does not match this build — e.g. a committed RON written before a
    /// `Mode` was added. Loud, at the door, rather than a silently misaligned distribution.
    pub fn validate(&self) -> Result<(), String> {
        if self.counts.len() != CONTEXT_COUNT {
            return Err(format!(
                "baseline prior has {} contexts but this build has {CONTEXT_COUNT}; regenerate it \
                 (`train prior`)",
                self.counts.len()
            ));
        }
        Ok(())
    }

    /// Laplace-smoothed `P(mode | context)`. A context the baseline never visited yields the uniform
    /// distribution, which is the honest statement of "we have no expectation here".
    fn prob(&self, context: usize, mode: usize) -> f64 {
        let row = &self.counts[context];
        let total: u32 = row.iter().sum();
        (f64::from(row[mode]) + ALPHA) / (f64::from(total) + ALPHA * MODE_COUNT as f64)
    }

    pub fn total_observations(&self) -> u64 {
        self.counts.iter().flat_map(|r| r.iter()).map(|c| u64::from(*c)).sum()
    }
}

/// Bayesian surprise, in **nats**: the witness-weighted KL divergence of the candidate's realised mode
/// distribution from the baseline prior, conditioned on context.
///
/// `D_KL(P_candidate ‖ P_prior) = Σ p·ln(p/q)`, averaged over contexts by how often the *witnessed*
/// decisions landed in each.
///
/// **Only contexts with at least [`MIN_CONTEXT_SAMPLES`] witnessed decisions contribute**, with the
/// weights renormalised over those. The plug-in KL is biased upward, catastrophically so for a thinly
/// sampled context, and because that bias tracks a candidate's context-occupancy histogram it does not
/// cancel — it would reward scattering decisions across many contexts. Refusing to estimate from too
/// little evidence removes the exploit at its root. See [`ALPHA`] for why symmetric smoothing, the obvious
/// alternative, makes it worse.
///
/// Unwitnessed decisions are excluded entirely: surprise the player did not see is not surprise.
/// Returns `0.0` when no context clears the evidence floor.
pub fn bayesian_surprise(trace: &EpisodeTrace, prior: &ModePrior) -> f64 {
    let mut counts = vec![[0u32; MODE_COUNT]; CONTEXT_COUNT];
    let mut witnessed_total = 0u64;
    for d in trace.decisions.iter().filter(|d| d.witnessed) {
        counts[d.context.key()][d.mode.index()] += 1;
        witnessed_total += 1;
    }
    if witnessed_total == 0 {
        return 0.0;
    }

    // Renormalise over the contexts that clear the evidence floor; the rest contribute nothing.
    let admissible: u64 = counts
        .iter()
        .map(|row| u64::from(row.iter().sum::<u32>()))
        .filter(|n| *n >= u64::from(MIN_CONTEXT_SAMPLES))
        .sum();
    if admissible == 0 {
        return 0.0;
    }

    let mut surprise = 0.0f64;
    for (ctx, row) in counts.iter().enumerate() {
        let n: u32 = row.iter().sum();
        if n < MIN_CONTEXT_SAMPLES {
            continue;
        }
        let weight = f64::from(n) / admissible as f64;
        let mut kl = 0.0f64;
        for (mode, &c) in row.iter().enumerate() {
            if c == 0 {
                continue; // 0*ln 0 = 0
            }
            let p = f64::from(c) / f64::from(n);
            let q = prior.prob(ctx, mode);
            kl += p * (p / q).ln();
        }
        surprise += weight * kl;
    }
    // KL is non-negative in exact arithmetic; clamp the float noise rather than let a -1e-17 leak out.
    surprise.max(0.0)
}

/// Squash unbounded surprise (nats) into `[0,1)` monotonically: `1 - e^{-S}`.
///
/// A hard cap (`S / ln MODE_COUNT`) would need an arbitrary ceiling and would saturate — every strongly
/// divergent brain would tie at 1.0, collapsing the selection pressure exactly where it matters. The
/// exponential squash never ties and needs no constant.
pub fn surprise_score(surprise_nats: f64) -> f32 {
    (1.0 - (-surprise_nats).exp()) as f32
}

// ── L: learnable (compression progress) ──────────────────────────────────────────────────────────

/// A first-order Markov model over mode transitions — the simplest thing that can be said to "understand"
/// a behaviour: *given what it just did, what will it do next?*
#[derive(Clone, Debug)]
pub struct MarkovModel {
    /// `counts[from][to]`.
    counts: Vec<[u32; MODE_COUNT]>,
}

impl MarkovModel {
    /// Fit on a trace's per-actor mode sequences.
    pub fn fit(trace: &EpisodeTrace) -> Self {
        let mut counts = vec![[0u32; MODE_COUNT]; MODE_COUNT];
        for seq in trace.sequences() {
            for pair in seq.windows(2) {
                counts[pair[0].index()][pair[1].index()] += 1;
            }
        }
        MarkovModel { counts }
    }

    /// Laplace-smoothed `P(to | from)`.
    fn prob(&self, from: usize, to: usize) -> f64 {
        let row = &self.counts[from];
        let total: u32 = row.iter().sum();
        (f64::from(row[to]) + ALPHA) / (f64::from(total) + ALPHA * MODE_COUNT as f64)
    }

    /// Total log-likelihood (nats) this model assigns to another trace's transitions.
    fn log_likelihood(&self, trace: &EpisodeTrace) -> (f64, usize) {
        let mut ll = 0.0f64;
        let mut n = 0usize;
        for seq in trace.sequences() {
            for pair in seq.windows(2) {
                ll += self.prob(pair[0].index(), pair[1].index()).ln();
                n += 1;
            }
        }
        (ll, n)
    }
}

/// The **null model**: a trace's own 0th-order marginal over modes, fitted on the very trace it scores.
/// Deliberately optimistic — the strongest "you learned nothing about the dynamics" baseline available.
fn marginal_log_likelihood(trace: &EpisodeTrace) -> (f64, usize) {
    let mut counts = [0u32; MODE_COUNT];
    let mut transitions: Vec<usize> = Vec::new();
    for seq in trace.sequences() {
        for pair in seq.windows(2) {
            counts[pair[1].index()] += 1;
            transitions.push(pair[1].index());
        }
    }
    let total: u32 = counts.iter().sum();
    if total == 0 {
        return (0.0, 0);
    }
    let denom = f64::from(total) + ALPHA * MODE_COUNT as f64;
    let ll: f64 = transitions
        .iter()
        .map(|&to| ((f64::from(counts[to]) + ALPHA) / denom).ln())
        .sum();
    (ll, transitions.len())
}

/// **Compression progress**, in `[0,1]`: how many nats a model of rollout `a` saves when predicting
/// rollout `b`, as a fraction of what `b`'s own marginal already costs.
///
/// This is the noisy-TV guard, and the *only* term that distinguishes "astonishing" from "random". A
/// coin-flipping brain has no transition structure, so a model fitted on `a` cannot beat `b`'s marginal
/// and `L → 0`. A brain with structure — *she always wards right before she regroups* — is compressible
/// after one viewing, and scores.
///
/// `a` and `b` must be two rollouts of the **same** candidate on different episode seeds. Returns `0.0`
/// when `b` has no transitions to predict (fewer than two decisions by any actor).
pub fn learnability(a: &EpisodeTrace, b: &EpisodeTrace) -> f32 {
    let (ll_null, n) = marginal_log_likelihood(b);
    if n == 0 || ll_null >= 0.0 {
        // `ll_null` is a sum of logs of probabilities, hence < 0 whenever n > 0. `>= 0` means n == 0.
        return 0.0;
    }
    let (ll_fitted, _) = MarkovModel::fit(a).log_likelihood(b);
    // Both are negative; `ll_fitted > ll_null` means the fitted model predicts `b` better.
    let saved = ll_fitted - ll_null;
    (saved / ll_null.abs()).clamp(0.0, 1.0) as f32
}

// ── W · S · L ────────────────────────────────────────────────────────────────────────────────────

/// The three factors, kept separate so a low score can be *explained* rather than merely observed. An
/// archive of these is what a human reviews before an elite is committed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fitness {
    pub witnessed: f32,
    pub surprise: f32,
    pub learnable: f32,
}

impl Fitness {
    /// `W · S · L`. Multiplicative, not additive: each factor is a *veto*. Behaviour nobody saw, or that
    /// nobody could learn, or that nobody found surprising, is worth exactly nothing — and a weighted sum
    /// would let a strong term buy off a zero one.
    pub fn score(&self) -> f32 {
        (self.witnessed * self.surprise * self.learnable).clamp(0.0, 1.0)
    }
}

/// Score a candidate from its two rollouts against the committed baseline prior.
///
/// Both rollouts feed the surprise term (twice the evidence, no extra simulation), and the pair feeds
/// learnability: fit on `a`, predict `b`.
pub fn fitness(a: &EpisodeTrace, b: &EpisodeTrace, prior: &ModePrior) -> Fitness {
    let witnessed = 0.5 * (witnessed_fraction(a) + witnessed_fraction(b));
    let nats = 0.5 * (bayesian_surprise(a, prior) + bayesian_surprise(b, prior));
    Fitness { witnessed, surprise: surprise_score(nats), learnable: learnability(a, b) }
}

// ── The behavioural minimal criterion ────────────────────────────────────────────────────────────

/// What actually happened in an episode, beyond the decisions taken. Fed to [`minimal_criterion`].
#[derive(Clone, Copy, Debug, Default)]
pub struct EpisodeOutcome {
    pub squad_size: u32,
    pub survivors: u32,
    /// Decisions in which a unit's brain chose **role work** over the shared cohesion/survival tail — the
    /// squad's agency signal. See [`is_squad_duty`].
    pub squad_duty_decisions: u32,
    /// Every crab despawn. **Not attributable to the squad**: the Smiley culls crabs, and a crab that
    /// delivers meat to its nest despawns. Evidence that the world was dynamic, not that the squad fought.
    pub crabs_killed: u32,
    pub crabs_alive: u32,
    pub unit_damage_taken: f32,
    pub cells_covered: u32,
    pub reachable_cells: u32,
    /// Non-empty when `sim_harness::liveness_violations` fired at any checkpoint.
    pub liveness_violations: u32,
    /// Ticks during which a player `MoveOrder` was standing on at least one unit — i.e. ticks in which the
    /// squad brain was *not* in control of movement, `unit_actions`, or `medic_heal`. Diagnostic: an
    /// evaluation in which this approaches the episode length is not evaluating the squad.
    pub ordered_ticks: u32,
    /// Peak stigmergy-field value seen at any checkpoint — the **saturation** guard. A near-zero
    /// evaporation rate makes a channel accumulate without bound; the genome `BOUNDS` floor evaporation to
    /// prevent it, and this catches any residual runaway. Sampled in `evaluate::rollout`.
    pub peak_field: f32,
    /// Max fraction of the floor at ≥ half the peak field value seen at any checkpoint — the **whole-map
    /// smear** guard. A flooded field is uniform (no gradient to navigate); the shipped game is sparse, so
    /// this stays small. `1.0` would mean the whole floor is saturated. Sampled in `evaluate::rollout`.
    pub field_flatness: f32,
}

/// Minimum fraction of the reachable map an episode must cover to count as "an encounter happened".
///
/// **Calibrated by measurement** (`train probe`), not chosen. `reachable_cells` counts *fine floor tiles*
/// — metres — of which a dungeon has ~3.5–4.5k, while a squad on a 60 s tour walks a few hundred. The
/// first guess, 10%, was unreachable and rejected the shipped game itself.
///
/// Observed with the authored brains and the synthetic player (`squad_ai::evaluate`):
///
/// | episode | coverage |
/// |---|---|
/// | 1800 ticks (30 s) | 4.1% |
/// | 3600 ticks (60 s) | 8.5% |
/// | idle squad, no player | ~0.15% |
///
/// 2% admits the shipped game with >2× headroom at the shortest useful episode, and still rejects a
/// do-nothing brain by a factor of ~25. The invariant that governs this constant: **the minimal criterion
/// must admit the game as shipped**, or the search is measuring the wrong thing
/// (`tests/search_calibration.rs`).
const MIN_COVERAGE: f32 = 0.02;

/// Max fraction of the floor at ≥ half the peak field value an episode may reach before its stigmergy
/// field counts as a degenerate **whole-map smear** — flooded uniform, with no gradient for agents to
/// climb, so the coordination the whole AI rests on is dead.
///
/// **Calibrated by `train probe`**, not chosen: the shipped worlds run at 0.3–0.5% flatness (a sharp peak
/// over sparse activity) across the three held-in seeds at 7200 ticks, so a 50% ceiling is a ~100× backstop
/// that only fires on a genuinely gradient-less field. `EpisodeOutcome::peak_field` is *recorded* (a
/// diagnostic `train probe` prints) but deliberately NOT gated: the `world_genome::BOUNDS` floor
/// evaporation, so the field cannot saturate unbounded, and a high-but-finite peak is an *intense* world —
/// exactly what the search is meant to discover, not a degenerate one to reject.
const FIELD_FLATNESS_CEILING: f32 = 0.5;

/// The **behavioural** minimal criterion: did a real encounter happen?
///
/// This is a hard gate, not a soft penalty. Skalse et al. ("Defining and Characterizing Reward Hacking",
/// arXiv:2209.13085) show that a hackable proxy stays hackable when you merely subtract a penalty from
/// it; the only reliable remedy is to *restrict the policy set*. So an episode that fails any clause is
/// discarded outright, never scored and down-weighted.
///
/// It is what rejects the degenerates `fitness` cannot see:
/// - **always-Flee** (a flat `Logistic` is the constant 0.5, so an all-zero genome passes both startup
///   guards) — no crab dies, the squad takes no damage, the map is never explored;
/// - **always-Wander** — same;
/// - a squad wipe, or a swarm extinction, which leave nothing to co-adapt against.
///
/// The clauses are *relational* — they depend on the opponent, not on the candidate alone. That is the
/// point: this is minimal-criterion coevolution, and "neither too easy nor too hard" is a statement about
/// a pairing (Wang et al., POET, arXiv:1901.01753 §3).
pub fn minimal_criterion(outcome: &EpisodeOutcome) -> Result<(), String> {
    if outcome.liveness_violations > 0 {
        return Err(format!("{} liveness violation(s)", outcome.liveness_violations));
    }
    if outcome.squad_size == 0 {
        return Err("no squad".into());
    }
    if outcome.survivors == 0 {
        return Err("squad was wiped — nothing survived to be watched".into());
    }
    if outcome.crabs_alive == 0 {
        return Err("swarm went extinct — nothing left to co-adapt against".into());
    }
    if outcome.crabs_killed == 0 {
        return Err("no crab died — the world was static".into());
    }
    // The agency clause. Everything above can be satisfied by the environment: the rifles auto-fire, the
    // synthetic player walks the squad into the nests, and crabs despawn for reasons the squad had no
    // part in. This is the only clause that asks whether the *brain under test* ever chose to fight, and
    // it is what rejects the low-structure coward (Flee/Regroup/FollowAnchor cycling) that `L` cannot see.
    if outcome.squad_duty_decisions == 0 {
        return Err(
            "the squad never once chose role work — it only followed, fled, and regrouped, so it was \
             carried through the episode rather than playing it"
                .into(),
        );
    }
    if outcome.unit_damage_taken <= 0.0 {
        return Err("no unit was ever hurt — nothing was at stake".into());
    }
    if outcome.reachable_cells == 0 {
        return Err("no reachable cells — degenerate dungeon".into());
    }
    let coverage = outcome.cells_covered as f32 / outcome.reachable_cells as f32;
    if coverage < MIN_COVERAGE {
        return Err(format!("covered {:.1}% of the map (< {:.0}%)", coverage * 100.0, MIN_COVERAGE * 100.0));
    }
    // Field-sanity (see [`FIELD_FLATNESS_CEILING`]): reject a flooded, gradient-less field. A hard gate, not
    // a penalty (Skalse et al.) — the primary defence is the genome `BOUNDS`; this is the residual backstop
    // for in-bounds knob combinations that still homogenize the field over the episode.
    if outcome.field_flatness > FIELD_FLATNESS_CEILING {
        return Err(format!(
            "field smeared: {:.0}% of the floor at >= half-peak (> {:.0}%) — a flooded, gradient-less field",
            outcome.field_flatness * 100.0,
            FIELD_FLATNESS_CEILING * 100.0
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(actor: ActorKind, fear: FearBucket) -> Context {
        Context { actor, fear, threat_known: false, ally_down: false, past_leash: false }
    }

    /// A trace of one actor cycling deterministically through `modes`, all witnessed.
    fn trace_of(modes: &[Mode]) -> EpisodeTrace {
        EpisodeTrace {
            decisions: modes
                .iter()
                .map(|&mode| Decision {
                    actor_id: 0,
                    context: ctx(ActorKind::Role(RoleId::Gunman), FearBucket::Calm),
                    mode,
                    witnessed: true,
                })
                .collect(),
        }
    }

    /// A deterministic pseudo-random mode walk — the "noisy TV".
    fn noisy_trace(seed: u64, n: usize) -> EpisodeTrace {
        let mut s = seed;
        let modes: Vec<Mode> = (0..n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                Mode::ALL[(s >> 33) as usize % MODE_COUNT]
            })
            .collect();
        trace_of(&modes)
    }

    #[test]
    fn duty_is_the_complement_of_the_shared_tail() {
        // The tail every role carries: survival, cohesion, drift, safety default. Picking only from it is
        // what "carried" means. This is the clause that rejects the low-structure coward `L` cannot see.
        for mode in [Mode::Flee, Mode::Regroup, Mode::FollowAnchor, Mode::Wander] {
            assert!(!is_squad_duty(mode), "{mode:?} is tail, not duty");
        }
        for mode in [Mode::Overwatch, Mode::Examine, Mode::TendWounded, Mode::SecureDoor, Mode::Ward] {
            assert!(is_squad_duty(mode), "{mode:?} is role work");
        }
        // Every offensive mode is also duty; duty is strictly broader.
        for mode in Mode::ALL {
            if is_squad_offensive(mode) {
                assert!(is_squad_duty(mode), "{mode:?}");
            }
        }
    }

    #[test]
    fn role_indices_are_dense() {
        for (i, role) in RoleId::ALL.iter().enumerate() {
            assert_eq!(ActorKind::Role(*role).index(), i);
        }
        assert_eq!(ActorKind::Smiley.index(), ACTOR_COUNT - 1);
    }

    #[test]
    fn context_keys_are_dense_and_unique() {
        let mut seen = vec![false; CONTEXT_COUNT];
        let actors: Vec<ActorKind> = RoleId::ALL
            .iter()
            .map(|r| ActorKind::Role(*r))
            .chain([ActorKind::Crab, ActorKind::Scout, ActorKind::Smiley])
            .collect();
        for actor in actors {
            for fear in [FearBucket::Calm, FearBucket::Wary, FearBucket::Panicked] {
                for flags in 0..8u8 {
                    let c = Context {
                        actor,
                        fear,
                        threat_known: flags & 1 != 0,
                        ally_down: flags & 2 != 0,
                        past_leash: flags & 4 != 0,
                    };
                    let k = c.key();
                    assert!(k < CONTEXT_COUNT, "key {k} out of range");
                    assert!(!seen[k], "duplicate context key {k}");
                    seen[k] = true;
                }
            }
        }
        assert!(seen.iter().all(|s| *s), "every context key must be reachable");
    }

    #[test]
    fn fear_buckets_partition_the_unit_interval() {
        assert_eq!(FearBucket::of(0.0), FearBucket::Calm);
        assert_eq!(FearBucket::of(0.32), FearBucket::Calm);
        assert_eq!(FearBucket::of(0.34), FearBucket::Wary);
        assert_eq!(FearBucket::of(0.65), FearBucket::Wary);
        assert_eq!(FearBucket::of(0.67), FearBucket::Panicked);
        assert_eq!(FearBucket::of(1.0), FearBucket::Panicked);
        // A non-finite drive is a bug elsewhere; classify it, never propagate it into a probability.
        assert_eq!(FearBucket::of(f32::NAN), FearBucket::Calm);
    }

    #[test]
    fn witness_is_the_fraction_the_player_could_see() {
        assert_eq!(witnessed_fraction(&EpisodeTrace::default()), 0.0);
        let mut t = trace_of(&[Mode::Engage, Mode::Engage, Mode::Engage, Mode::Engage]);
        t.decisions[0].witnessed = false;
        t.decisions[1].witnessed = false;
        assert_eq!(witnessed_fraction(&t), 0.5);
    }

    #[test]
    fn surprise_is_near_zero_against_the_prior_that_generated_it() {
        // A candidate that behaves exactly like the baseline is, by definition, not surprising. "Near"
        // zero, not zero: smoothing keeps a small floor (see `smoothing_bias_shrinks_as_the_prior_gains_
        // evidence`), which is common to every candidate and therefore cancels in the search.
        let cycle = [Mode::Overwatch, Mode::Engage];
        let baseline = trace_of(&cycle.repeat(64));
        let prior = ModePrior::from_traces(&vec![baseline.clone(); 40]);
        let s = bayesian_surprise(&baseline, &prior);
        assert!(s < 0.02, "self-surprise should be ~0, got {s}");
    }

    #[test]
    fn smoothing_bias_shrinks_as_the_prior_gains_evidence() {
        // Documents the one place ALPHA is visible in the output. Self-surprise decays toward 0 as the
        // baseline prior accumulates observations; it never reaches it. This is why `train prior` must
        // sweep enough episodes, and why the ALPHA choice (KT/Jeffreys, not Laplace) matters.
        let cycle = [Mode::Overwatch, Mode::Engage];
        let baseline = trace_of(&cycle.repeat(64));
        let mut previous = f64::INFINITY;
        for reps in [1usize, 5, 20, 100] {
            let prior = ModePrior::from_traces(&vec![baseline.clone(); reps]);
            let s = bayesian_surprise(&baseline, &prior);
            assert!(s < previous, "self-surprise must fall as evidence grows: {s} !< {previous}");
            assert!(s > 0.0, "smoothing keeps a strictly positive floor");
            previous = s;
        }
    }

    #[test]
    fn surprise_rises_when_a_candidate_departs_from_the_prior() {
        let baseline = trace_of(&[Mode::Overwatch; 64]);
        let prior = ModePrior::from_traces(&[baseline.clone()]);
        let same = bayesian_surprise(&trace_of(&[Mode::Overwatch; 64]), &prior);
        let different = bayesian_surprise(&trace_of(&[Mode::Ward; 64]), &prior);
        assert!(different > same, "departing from the prior must score higher: {different} vs {same}");
        assert!(surprise_score(different) > surprise_score(same));
        // The squash is monotone and bounded.
        assert!((0.0..1.0).contains(&surprise_score(different)));
        assert_eq!(surprise_score(0.0), 0.0);
    }

    #[test]
    fn thinly_visited_contexts_contribute_no_surprise() {
        // The context-scattering exploit. A brain that decides *once* in each of many contexts would,
        // under a raw plug-in KL, harvest up to `ln 24 = 3.18` nats of pure estimator bias from every one
        // of them — buying a high `S` with no behaviour at all. Below MIN_CONTEXT_SAMPLES a context is
        // simply not evidence.
        let prior = ModePrior::from_traces(&[trace_of(&[Mode::Overwatch; 256])]);
        let scattered = EpisodeTrace {
            decisions: (0..64)
                .map(|i| Decision {
                    actor_id: i,
                    context: Context {
                        actor: ActorKind::Role(RoleId::ALL[(i % 5) as usize]),
                        fear: [FearBucket::Calm, FearBucket::Wary, FearBucket::Panicked][(i % 3) as usize],
                        threat_known: i % 2 == 0,
                        ally_down: i % 4 < 2,
                        past_leash: i % 8 < 4,
                    },
                    mode: Mode::Ward,
                    witnessed: true,
                })
                .collect(),
        };
        // Every context holds at most a couple of decisions.
        assert_eq!(bayesian_surprise(&scattered, &prior), 0.0, "thin evidence must score nothing");

        // The same total decisions, concentrated in ONE context, is genuinely surprising.
        let concentrated = trace_of(&[Mode::Ward; 64]);
        assert!(bayesian_surprise(&concentrated, &prior) > 1.0);
    }

    #[test]
    fn surprise_ignores_what_the_player_did_not_see() {
        // The inverted Hunicke constraint, at the level of a single term: a wildly divergent brain whose
        // every decision happened in fog is exactly as surprising as no brain at all.
        let prior = ModePrior::from_traces(&[trace_of(&[Mode::Overwatch; 32])]);
        let mut unseen = trace_of(&[Mode::Ward; 32]);
        for d in &mut unseen.decisions {
            d.witnessed = false;
        }
        assert_eq!(bayesian_surprise(&unseen, &prior), 0.0);
    }

    #[test]
    fn a_structured_behaviour_is_learnable() {
        // `a` and `b` are two rollouts of the same deterministic cycle — "she always wards right before
        // she regroups". A transition model fitted on `a` predicts `b` far better than `b`'s own
        // marginal does, so the surprise is *compressible after one viewing*: exactly the wow condition.
        let cycle = [Mode::Ward, Mode::Regroup, Mode::Overwatch, Mode::Engage];
        let a = trace_of(&cycle.repeat(64));
        let b = trace_of(&cycle.repeat(64));
        let l = learnability(&a, &b);
        assert!(l > 0.6, "a deterministic cycle must be highly learnable, got {l}");
    }

    #[test]
    fn learnability_grows_with_evidence_but_never_reaches_one() {
        // The smoothing floor again: a perfectly deterministic behaviour approaches L = 1 from below as
        // the episode lengthens. Pinned so a future ALPHA change is a visible, deliberate act.
        let cycle = [Mode::Ward, Mode::Regroup, Mode::Overwatch, Mode::Engage];
        let mut previous = 0.0f32;
        for reps in [8usize, 32, 128] {
            let a = trace_of(&cycle.repeat(reps));
            let b = trace_of(&cycle.repeat(reps));
            let l = learnability(&a, &b);
            assert!(l > previous, "L must grow with evidence: {l} !> {previous}");
            assert!(l < 1.0, "smoothing keeps L strictly below 1");
            previous = l;
        }
    }

    #[test]
    fn the_noisy_tv_is_not_learnable() {
        // THE guard. A brain that flips a coin every think is maximally surprising and worth nothing.
        // A model fitted on one of its rollouts cannot beat the other's marginal.
        let a = noisy_trace(0xA11CE, 600);
        let b = noisy_trace(0xBEEF, 600);
        let l = learnability(&a, &b);
        assert!(l < 0.05, "random behaviour must not be learnable, got {l}");

        // ...and it *would* have won on surprise alone, which is precisely why L exists.
        let prior = ModePrior::from_traces(&[trace_of(&[Mode::Overwatch; 600])]);
        let noisy_surprise = surprise_score(bayesian_surprise(&a, &prior));
        assert!(noisy_surprise > 0.5, "the noisy TV is surprising ({noisy_surprise}) — but unlearnable");

        let f = fitness(&a, &b, &prior);
        assert!(f.score() < 0.05, "W·S·L must veto the noisy TV, got {:?}", f);
    }

    #[test]
    fn fitness_is_multiplicative_so_any_zero_factor_vetoes() {
        // A weighted sum would let a strong surprise term buy off invisibility. A product cannot.
        let f = Fitness { witnessed: 0.0, surprise: 1.0, learnable: 1.0 };
        assert_eq!(f.score(), 0.0);
        let f = Fitness { witnessed: 1.0, surprise: 0.0, learnable: 1.0 };
        assert_eq!(f.score(), 0.0);
        let f = Fitness { witnessed: 1.0, surprise: 1.0, learnable: 0.0 };
        assert_eq!(f.score(), 0.0);
        let f = Fitness { witnessed: 0.5, surprise: 0.5, learnable: 0.5 };
        assert!((f.score() - 0.125).abs() < 1e-6);
    }

    #[test]
    fn learnability_of_an_empty_or_singleton_trace_is_zero() {
        assert_eq!(learnability(&EpisodeTrace::default(), &EpisodeTrace::default()), 0.0);
        assert_eq!(learnability(&trace_of(&[Mode::Wander]), &trace_of(&[Mode::Wander])), 0.0);
    }

    #[test]
    fn sequences_are_threaded_per_actor_not_globally() {
        // Two actors interleaved in tick order must not produce cross-actor transitions.
        let g = ctx(ActorKind::Role(RoleId::Gunman), FearBucket::Calm);
        let m = ctx(ActorKind::Role(RoleId::Medic), FearBucket::Calm);
        let trace = EpisodeTrace {
            decisions: vec![
                Decision { actor_id: 0, context: g, mode: Mode::Overwatch, witnessed: true },
                Decision { actor_id: 1, context: m, mode: Mode::TendWounded, witnessed: true },
                Decision { actor_id: 0, context: g, mode: Mode::Engage, witnessed: true },
                Decision { actor_id: 1, context: m, mode: Mode::Regroup, witnessed: true },
            ],
        };
        let seqs = trace.sequences();
        assert_eq!(seqs, vec![
            vec![Mode::Overwatch, Mode::Engage],
            vec![Mode::TendWounded, Mode::Regroup]
        ]);
        // Exactly two transitions, one per actor — never Overwatch→TendWounded.
        let (_, n) = MarkovModel::fit(&trace).log_likelihood(&trace);
        assert_eq!(n, 2);
    }

    #[test]
    fn prior_validates_its_shape() {
        let good = ModePrior::default();
        assert!(good.validate().is_ok());
        let bad = ModePrior { counts: vec![[0; MODE_COUNT]; CONTEXT_COUNT - 1] };
        assert!(bad.validate().is_err(), "a stale committed prior must be rejected at the door");
    }

    fn healthy() -> EpisodeOutcome {
        EpisodeOutcome {
            squad_size: 5,
            survivors: 3,
            squad_duty_decisions: 20,
            crabs_killed: 12,
            crabs_alive: 30,
            unit_damage_taken: 40.0,
            cells_covered: 50,
            reachable_cells: 200,
            liveness_violations: 0,
            ordered_ticks: 0,
            // A sparse, non-degenerate field: a modest peak over a small fraction of the floor.
            peak_field: 8.0,
            field_flatness: 0.15,
        }
    }

    #[test]
    fn minimal_criterion_admits_a_real_encounter() {
        assert!(minimal_criterion(&healthy()).is_ok());
    }

    #[test]
    fn minimal_criterion_rejects_a_smeared_field() {
        // A flooded, gradient-less field is a hard reject; a shipped-range flatness passes with headroom
        // (shipped is 0.3–0.5%, healthy() is 0.15, the ceiling is 0.5).
        let smeared = EpisodeOutcome { field_flatness: 0.9, ..healthy() };
        assert!(minimal_criterion(&smeared).is_err(), "a flooded, gradient-less field must be rejected");
        assert!(minimal_criterion(&EpisodeOutcome { field_flatness: 0.45, ..healthy() }).is_ok());
    }

    #[test]
    fn minimal_criterion_rejects_the_degenerates_fitness_cannot_see() {
        // always-Flee / always-Wander: nothing dies, nobody is hurt, nothing is explored.
        let idle = EpisodeOutcome { crabs_killed: 0, unit_damage_taken: 0.0, cells_covered: 2, ..healthy() };
        assert!(minimal_criterion(&idle).is_err());

        let wiped = EpisodeOutcome { survivors: 0, ..healthy() };
        assert!(minimal_criterion(&wiped).is_err());

        let extinct = EpisodeOutcome { crabs_alive: 0, ..healthy() };
        assert!(minimal_criterion(&extinct).is_err());

        let untouched = EpisodeOutcome { unit_damage_taken: 0.0, ..healthy() };
        assert!(minimal_criterion(&untouched).is_err());

        // Derived from MIN_COVERAGE so recalibrating the constant cannot silently un-test this clause.
        let reachable = 4000u32;
        let cooped = EpisodeOutcome {
            cells_covered: (MIN_COVERAGE * reachable as f32) as u32 - 1,
            reachable_cells: reachable,
            ..healthy()
        };
        assert!(minimal_criterion(&cooped).is_err(), "just under MIN_COVERAGE must be rejected");
        let walked = EpisodeOutcome {
            cells_covered: (MIN_COVERAGE * reachable as f32) as u32 + 1,
            reachable_cells: reachable,
            ..healthy()
        };
        assert!(minimal_criterion(&walked).is_ok(), "just over MIN_COVERAGE must be admitted");

        let broken = EpisodeOutcome { liveness_violations: 1, ..healthy() };
        assert!(minimal_criterion(&broken).is_err());

        // THE agency clause. Everything else in `healthy()` is satisfiable by the environment alone —
        // rifles auto-fire, the synthetic player walks the squad into the nests, and crabs despawn for
        // reasons the squad had no part in. A brain that never chooses to fight must still be rejected.
        let carried = EpisodeOutcome { squad_duty_decisions: 0, ..healthy() };
        assert!(
            minimal_criterion(&carried).is_err(),
            "a squad that only ever followed/fled/regrouped was carried, not playing"
        );
    }

    #[test]
    fn a_constant_brain_is_not_learnable() {
        // Pins the claim in the module docs, which was originally stated BACKWARDS. A brain that always
        // picks one mode is not caught by `minimal_criterion` — it is caught by `L`, and scores exactly 0:
        // for a constant sequence the fitted transition row and the marginal are the same estimator, so no
        // nats are saved. This is also what Schmidhuber's principle demands (a fully predictable stream
        // yields no compression *progress*).
        let a = trace_of(&[Mode::Flee; 256]);
        let b = trace_of(&[Mode::Flee; 256]);
        assert_eq!(learnability(&a, &b), 0.0, "a constant brain saves no nats over the marginal");

        // ...and it is wildly divergent from a baseline that never flees, so S alone would love it.
        let prior = ModePrior::from_traces(&[trace_of(&[Mode::Overwatch; 256])]);
        assert!(surprise_score(bayesian_surprise(&a, &prior)) > 0.5);
        assert_eq!(fitness(&a, &b, &prior).score(), 0.0, "W·S·L must veto the constant brain");
    }

    #[test]
    fn the_prior_ignores_unwitnessed_decisions() {
        // The conditioning event must match `bayesian_surprise`, which filters `p` to witnessed decisions.
        // Otherwise a candidate identical to the baseline scores spurious self-surprise on `Flee`.
        let mut trace = trace_of(&[Mode::Overwatch, Mode::Flee, Mode::Overwatch, Mode::Flee]);
        trace.decisions[1].witnessed = false;
        trace.decisions[3].witnessed = false;
        let prior = ModePrior::from_traces(&[trace]);
        assert_eq!(prior.total_observations(), 2, "only the witnessed decisions form the expectation");
    }
}
