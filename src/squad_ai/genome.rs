//! The **genome**: a behaviour repertoire viewed as a flat, mutable parameter vector.
//!
//! The offline behaviour search does not learn a neural policy. It searches the *authored* dual-utility
//! brain's own parameters — every [`Curve`]'s constants plus each [`Behavior`]'s rank — leaving the
//! structure (which modes exist, which [`Input`] each consideration reads, where it aims) fixed. This is
//! the representation choice that keeps three properties the project depends on:
//!
//! 1. **No new dependency.** ~100 `f32`s, mutated with the in-tree seeded ChaCha8 (`crate::rng`).
//! 2. **Every trained artifact is readable.** A genome decodes to `Vec<Behavior>`, which serialises to
//!    the same `roles.ron` a human authors — so an elite is a *diff*, not opaque weights. This is the
//!    practical answer to reward hacking (Skalse et al., "Defining and Characterizing Reward Hacking",
//!    arXiv:2209.13085): you can read what the optimiser found and reject it.
//! 3. **The engine's safety guards survive.** `validate_unconditional_default` and
//!    `validate_rank_ladder` still gate every candidate — they become the *minimal criterion* of the
//!    search (Wang et al., POET, arXiv:1901.01753 §2.2), so an infeasible brain is rejected at the door
//!    rather than degraded into a fallback.
//!
//! **Ranks mutate by permutation, never by assignment.** [`mutate`] swaps two behaviours' ranks, so the
//! multiset of ranks is invariant and the strict ladder holds *by construction*. The optimiser therefore
//! cannot reintroduce the rank-tie mode thrash that `validate_rank_ladder` exists to catch.
//!
//! **Parameters mutate relative to their authored magnitude.** A single absolute sigma cannot serve both
//! a drive curve (inputs in `[0,1]`) and a distance curve (inputs up to `NO_TARGET_DIST = 999`). Each
//! param is perturbed by `N(0, sigma · (|authored| + SCALE_FLOOR))` and clamped to a band of the same
//! width around its authored value, so the search explores in units the author chose.

use rand_chacha::ChaCha8Rng;

use crate::ai::utility::{validate_unconditional_default, Behavior, Curve};
use crate::rng::DetRng;
use crate::squad_ai::role::{validate_rank_ladder, RoleId};

/// Minimum mutation scale, so a parameter authored at exactly `0.0` can still move. Without it a zero
/// intercept (`Linear { m, b: 0.0 }`) would be frozen for the whole search.
const SCALE_FLOOR: f32 = 0.25;

/// How many mutation-scales a parameter may drift from its authored value. Bounds the search to brains
/// a designer would recognise, and keeps `Logistic { k }` out of the range where `exp` saturates so
/// hard that a curve becomes a constant by accident rather than by choice.
const BAND: f32 = 4.0;

/// A repertoire's free parameters, flattened. Layout is a deterministic walk: behaviours in list order,
/// each behaviour's considerations in list order, each curve's constants in declaration order
/// (`Linear{m,b}` → 2, `Logistic{k,x0}` → 2, `Step{threshold,below,above}` → 3).
///
/// A `Genome` is meaningless without the template repertoire it was encoded from — it carries values,
/// not structure. [`decode`] reunites them and fails loudly on any mismatch.
#[derive(Clone, Debug, PartialEq)]
pub struct Genome {
    pub params: Vec<f32>,
    pub ranks: Vec<u8>,
}

/// Whether a parameter may cross zero under mutation.
///
/// **A slope's sign is structure, not a value.** `Linear{m}` and `Logistic{k}` set a consideration's
/// *monotonicity direction*: "more fear ⇒ more likely to flee" versus its exact inverse. Flipping one is
/// not tuning, it is authoring a different behaviour — precisely what this genome excludes by fixing
/// structure and freeing values.
///
/// It is also load-bearing for feasibility. `utility::guaranteed_floor` can only bound a `Linear` from
/// below when `m >= 0` (a negative slope over an unbounded input — a distance runs to `NO_TARGET_DIST`
/// — guarantees nothing), and likewise a `Logistic` when `k >= 0`. Both shared unconditional tails,
/// `wander()` and `follow_anchor()`, are authored with `m = 0.0`. A symmetric Gaussian kick sends `m`
/// negative half the time, so an unlocked mutation destroys *both* tails' guaranteed floor in ~1 role in
/// 4, and a five-role squad survives about one time in a thousand. Measured: 0 of 32 children feasible.
/// A parameter that lives in **utility space** (the curve's output), which `Curve::eval` clamps to
/// `[0,1]`. Anything outside that range is behaviourally identical to the clamped value, so the search
/// would wander flat plateaus and the trained RON would read as nonsense.
///
/// Measured on a first archive before this existed: **46 of 59** `Step` arms and **13 of 43** `Linear`
/// intercepts landed outside `[0,1]`. One elite carried `Step { below: -0.122, above: -0.135 }` — both
/// arms clamp to 0, so the consideration was the constant 0 and had *silently deleted its behaviour*.
///
/// Note this does not apply to `Step::threshold` or `Logistic::x0`, which live in **input** space and are
/// legitimately unbounded (a distance runs to `NO_TARGET_DIST = 999`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ParamKind {
    /// Magnitude is free within the band; the sign is pinned to the authored one.
    SignLocked,
    /// Clamped to `[0,1]` — a utility-space value.
    UnitRange,
    Free,
}

/// Read one curve's constants into `out`, and its parameter kinds into `kinds`.
fn push_curve(curve: Curve, out: &mut Vec<f32>, kinds: &mut Vec<ParamKind>) {
    match curve {
        // `m` sets the direction; `b` is the utility intercept.
        Curve::Linear { m, b } => {
            out.extend_from_slice(&[m, b]);
            kinds.extend_from_slice(&[ParamKind::SignLocked, ParamKind::UnitRange]);
        }
        // `k` sets the direction; `x0` is an input-space midpoint and is legitimately unbounded.
        Curve::Logistic { k, x0 } => {
            out.extend_from_slice(&[k, x0]);
            kinds.extend_from_slice(&[ParamKind::SignLocked, ParamKind::Free]);
        }
        // A `Step` only ever emits one of its two arms, so `guaranteed_floor` bounds it regardless of
        // sign. `threshold` is input-space; both arms are utility-space.
        Curve::Step { threshold, below, above } => {
            out.extend_from_slice(&[threshold, below, above]);
            kinds.extend_from_slice(&[ParamKind::Free, ParamKind::UnitRange, ParamKind::UnitRange]);
        }
    }
}

/// The mutation kind of every parameter, in genome order. A pure function of the template's structure.
fn param_kinds(behaviors: &[Behavior]) -> Vec<ParamKind> {
    let mut params = Vec::new();
    let mut kinds = Vec::new();
    for behavior in behaviors {
        for consideration in &behavior.considerations {
            push_curve(consideration.curve, &mut params, &mut kinds);
        }
    }
    kinds
}

/// Rebuild a curve of the same *variant* as `template` from the next constants in `src`, advancing `at`.
/// The variant is never read from the genome — structure is fixed, only values move.
fn pull_curve(template: Curve, src: &[f32], at: &mut usize) -> Result<Curve, String> {
    let need = curve_arity(template);
    let end = *at + need;
    let slice = src.get(*at..end).ok_or_else(|| {
        format!("genome truncated: curve at param {at} needs {need} values, {} remain", src.len() - *at)
    })?;
    *at = end;
    Ok(match template {
        Curve::Linear { .. } => Curve::Linear { m: slice[0], b: slice[1] },
        Curve::Logistic { .. } => Curve::Logistic { k: slice[0], x0: slice[1] },
        Curve::Step { .. } => Curve::Step { threshold: slice[0], below: slice[1], above: slice[2] },
    })
}

/// Number of free constants a curve variant carries.
const fn curve_arity(curve: Curve) -> usize {
    match curve {
        Curve::Linear { .. } | Curve::Logistic { .. } => 2,
        Curve::Step { .. } => 3,
    }
}

/// Flatten a repertoire into its free parameters.
pub fn encode(behaviors: &[Behavior]) -> Genome {
    let mut params = Vec::new();
    let mut kinds = Vec::new();
    let mut ranks = Vec::with_capacity(behaviors.len());
    for behavior in behaviors {
        ranks.push(behavior.rank);
        for consideration in &behavior.considerations {
            push_curve(consideration.curve, &mut params, &mut kinds);
        }
    }
    Genome { params, ranks }
}

/// Rebuild a repertoire by laying `genome`'s values over `template`'s structure.
///
/// Every shape mismatch is an `Err`, never a partial or padded decode: a genome that does not fit its
/// template is a programming error, and silently tolerating it would produce a brain no one authored.
pub fn decode(template: &[Behavior], genome: &Genome) -> Result<Vec<Behavior>, String> {
    if genome.ranks.len() != template.len() {
        return Err(format!(
            "genome has {} ranks but the template has {} behaviours",
            genome.ranks.len(),
            template.len()
        ));
    }
    let mut at = 0usize;
    let mut out = template.to_vec();
    for (behavior, &rank) in out.iter_mut().zip(&genome.ranks) {
        behavior.rank = rank;
        for consideration in &mut behavior.considerations {
            consideration.curve = pull_curve(consideration.curve, &genome.params, &mut at)?;
        }
    }
    if at != genome.params.len() {
        return Err(format!(
            "genome has {} params but the template consumes {at}",
            genome.params.len()
        ));
    }
    Ok(out)
}

/// A standard normal draw (Box–Muller). `unit()` yields `[0, 1)`, so `1.0 - unit()` moves it to `(0, 1]`
/// and keeps `ln` finite.
fn gaussian(rng: &mut ChaCha8Rng) -> f32 {
    let u1 = 1.0 - rng.unit();
    let u2 = rng.unit();
    let r = (-2.0 * u1.ln()).sqrt();
    (r * (std::f64::consts::TAU * u2).cos()) as f32
}

/// Perturb a genome: every parameter gets a scale-relative Gaussian kick, clamped to a band around its
/// **authored** value, and with probability `rank_swap_p` two behaviours exchange ranks.
///
/// The authored genome is derived from `template`, never passed in, so the band's origin cannot drift:
/// seeding it from the parent instead would let bands ratchet outward across generations until every
/// curve saturated.
///
/// Sign-locked parameters ([`ParamKind::SignLocked`] — a `Linear`'s slope and a `Logistic`'s steepness)
/// keep their authored sign. See that type for why: a sign flip is a *structural* change, and unlocking
/// it makes essentially every child infeasible.
pub fn mutate(
    template: &[Behavior],
    parent: &Genome,
    sigma: f32,
    rank_swap_p: f64,
    rng: &mut ChaCha8Rng,
) -> Result<Genome, String> {
    let authored = encode(template);
    let kinds = param_kinds(template);
    if authored.params.len() != parent.params.len() || authored.ranks.len() != parent.ranks.len() {
        return Err(format!(
            "mutate: parent genome ({} params, {} ranks) does not fit the template ({} params, {} ranks)",
            parent.params.len(),
            parent.ranks.len(),
            authored.params.len(),
            authored.ranks.len()
        ));
    }

    let mut params = Vec::with_capacity(parent.params.len());
    for ((&p, &origin), &kind) in parent.params.iter().zip(&authored.params).zip(&kinds) {
        let scale = origin.abs() + SCALE_FLOOR;
        let moved = p + gaussian(rng) * sigma * scale;
        let (mut lo, mut hi) = (origin - BAND * scale, origin + BAND * scale);
        match kind {
            ParamKind::SignLocked => {
                // Closed half-line of the authored sign. An authored 0.0 counts as non-negative, which is
                // what keeps `guaranteed_floor`'s `m >= 0` / `k >= 0` branches reachable for the shared
                // unconditional tails (`wander`, `follow_anchor`), both authored with `m = 0.0`.
                if origin >= 0.0 {
                    lo = lo.max(0.0);
                } else {
                    hi = hi.min(0.0);
                }
            }
            // Utility space: outside [0,1] the curve's own clamp makes the value non-identifiable.
            ParamKind::UnitRange => {
                lo = lo.max(0.0);
                hi = hi.min(1.0);
            }
            ParamKind::Free => {}
        }
        params.push(moved.clamp(lo, hi));
    }

    // Rank mutation is a transposition, so `ranks` stays a permutation of the authored multiset and
    // `validate_rank_ladder` cannot fail because of it. Creature brains deliberately tie ranks, and a
    // transposition preserves ties too. A repertoire of fewer than two behaviours has no pair to swap.
    let mut ranks = parent.ranks.clone();
    if ranks.len() >= 2 && rng.unit() < rank_swap_p {
        let i = rng.below(ranks.len());
        let mut j = rng.below(ranks.len() - 1);
        if j >= i {
            j += 1; // uniform over the ranks != i, without rejection sampling
        }
        ranks.swap(i, j);
    }

    Ok(Genome { params, ranks })
}

/// The **genome-level minimal criterion**: can this brain even be run?
///
/// Reuses the engine's own startup guards rather than restating them, so a candidate the search admits
/// is exactly a candidate the shipped game would load. Cheap (no simulation), so it screens mutants
/// before any rollout is paid for. The *behavioural* half of the minimal criterion — "a real encounter
/// happened" — is evaluated after a rollout.
pub fn is_feasible(role: RoleId, template: &[Behavior], genome: &Genome) -> Result<(), String> {
    let behaviors = decode(template, genome)?;
    validate_unconditional_default(&behaviors, &format!("role {role:?}"))?;
    validate_rank_ladder(role, &behaviors)?;
    Ok(())
}

/// The creature-side minimal criterion. Same unconditional-default guard, **no rank ladder**: creature
/// brains deliberately share ranks (`Chase` ties `HuntBlood` so the stronger pull wins; `Latch` ties
/// `SeekMeat`), and `decide`'s weighted-random pick within the bucket is what makes a swarm read as
/// varied rather than as a single organism. Imposing the role invariant here would forbid the shipped
/// brains. `mutate`'s rank *transposition* preserves those ties, so the search cannot break them either.
pub fn is_feasible_creature(who: &str, template: &[Behavior], genome: &Genome) -> Result<(), String> {
    let behaviors = decode(template, genome)?;
    validate_unconditional_default(&behaviors, who)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::seeded;
    use crate::squad_ai::role::default_behaviors_for_test;

    /// Curve constants, in genome order — the observable a round trip must preserve.
    fn curves(behaviors: &[Behavior]) -> Vec<Curve> {
        behaviors.iter().flat_map(|b| b.considerations.iter().map(|c| c.curve)).collect()
    }

    #[test]
    fn encode_decode_round_trips_every_role() {
        for role in RoleId::ALL {
            let template = default_behaviors_for_test(role);
            let genome = encode(&template);
            let rebuilt = decode(&template, &genome).expect("round trip");
            assert_eq!(curves(&template), curves(&rebuilt), "{role:?} curves");
            let ranks: Vec<u8> = template.iter().map(|b| b.rank).collect();
            let rebuilt_ranks: Vec<u8> = rebuilt.iter().map(|b| b.rank).collect();
            assert_eq!(ranks, rebuilt_ranks, "{role:?} ranks");
        }
    }

    #[test]
    fn decode_rejects_a_mismatched_genome() {
        let template = default_behaviors_for_test(RoleId::Medic);
        let mut genome = encode(&template);
        genome.params.push(0.0);
        assert!(decode(&template, &genome).is_err(), "extra param must be rejected, not ignored");

        let mut short = encode(&template);
        short.params.pop();
        assert!(decode(&template, &short).is_err(), "missing param must be rejected, not padded");

        let mut ranks = encode(&template);
        ranks.ranks.pop();
        assert!(decode(&template, &ranks).is_err(), "rank count must match behaviour count");
    }

    #[test]
    fn mutation_is_deterministic_under_a_seed() {
        let template = default_behaviors_for_test(RoleId::Gunman);
        let authored = encode(&template);
        let a = {
            let mut r = seeded(42);
            mutate(&template, &authored, 0.2, 0.3, &mut r).expect("mutate")
        };
        let b = {
            let mut r = seeded(42);
            mutate(&template, &authored, 0.2, 0.3, &mut r).expect("mutate")
        };
        assert_eq!(a, b);
    }

    #[test]
    fn mutation_moves_parameters_but_stays_in_band() {
        let template = default_behaviors_for_test(RoleId::Psionic);
        let authored = encode(&template);
        let mut rng = seeded(7);
        let child = mutate(&template, &authored, 0.5, 0.0, &mut rng).expect("mutate");
        assert_ne!(child.params, authored.params, "a 0.5-sigma kick must move something");
        for (&p, &origin) in child.params.iter().zip(&authored.params) {
            let scale = origin.abs() + SCALE_FLOOR;
            assert!(
                p >= origin - BAND * scale && p <= origin + BAND * scale,
                "param {p} escaped the band around {origin}"
            );
            assert!(p.is_finite(), "mutation produced a non-finite parameter");
        }
    }

    #[test]
    fn rank_mutation_is_a_permutation_so_the_ladder_survives() {
        // The whole point of swapping rather than assigning: `validate_rank_ladder` can never fail, so
        // the optimiser cannot reintroduce the rank-tie mode thrash it exists to catch.
        let template = default_behaviors_for_test(RoleId::Engineer);
        let authored = encode(&template);
        let mut rng = seeded(3);
        let mut sorted_authored = authored.ranks.clone();
        sorted_authored.sort_unstable();

        let mut swapped_at_least_once = false;
        let mut parent = authored.clone();
        for _ in 0..64 {
            // sigma = 0 isolates the rank operator; rank_swap_p = 1.0 forces a swap every step.
            let child = mutate(&template, &parent, 0.0, 1.0, &mut rng).expect("mutate");
            let mut sorted_child = child.ranks.clone();
            sorted_child.sort_unstable();
            assert_eq!(sorted_authored, sorted_child, "ranks must stay a permutation");
            swapped_at_least_once |= child.ranks != authored.ranks;
            let decoded = decode(&template, &child).expect("decode");
            assert!(
                validate_rank_ladder(RoleId::Engineer, &decoded).is_ok(),
                "a permuted ladder is still strict"
            );
            parent = child;
        }
        assert!(swapped_at_least_once, "rank_swap_p = 1.0 must actually swap");
    }

    #[test]
    fn utility_space_parameters_never_leave_the_identifiable_range() {
        // `Curve::eval` clamps its output to [0,1], so a `Step` arm at 1.16 is exactly a `Step` arm at
        // 1.0 — a flat plateau the search would wander for free. Before this rule, 46/59 trained Step arms
        // sat outside the range, and one elite silently disabled a behaviour with two negative arms.
        for role in RoleId::ALL {
            let template = default_behaviors_for_test(role);
            let mut parent = encode(&template);
            let mut rng = seeded(11);
            for _ in 0..24 {
                parent = mutate(&template, &parent, 0.8, 0.0, &mut rng).expect("mutate");
                for behavior in decode(&template, &parent).expect("decode") {
                    for c in behavior.considerations {
                        match c.curve {
                            Curve::Linear { b, .. } => assert!((0.0..=1.0).contains(&b), "b = {b}"),
                            Curve::Step { below, above, .. } => {
                                assert!((0.0..=1.0).contains(&below), "below = {below}");
                                assert!((0.0..=1.0).contains(&above), "above = {above}");
                            }
                            // x0 is input space (distances reach NO_TARGET_DIST) — legitimately unbounded.
                            Curve::Logistic { .. } => {}
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_authored_repertoires_are_feasible() {
        // The minimal criterion must admit the shipped brains, or it is measuring the wrong thing.
        for role in RoleId::ALL {
            let template = default_behaviors_for_test(role);
            let genome = encode(&template);
            assert!(is_feasible(role, &template, &genome).is_ok(), "{role:?} default must be feasible");
        }
    }

    /// The same curve variant, parameterised so its `guaranteed_floor` is 0 — i.e. perception can always
    /// switch it off. Mirrors the reasoning in `utility::guaranteed_floor`.
    fn gated(curve: Curve) -> Curve {
        match curve {
            // Non-negative slope → floor is the intercept.
            Curve::Linear { .. } => Curve::Linear { m: 0.0, b: 0.0 },
            // Non-negative k → floor is eval(0) = 1/(1 + e^(k·x0)), vanishing for large positive k·x0.
            Curve::Logistic { .. } => Curve::Logistic { k: 10.0, x0: 10.0 },
            // A step only ever emits one of its arms.
            Curve::Step { .. } => Curve::Step { threshold: 0.5, below: 0.0, above: 0.0 },
        }
    }

    #[test]
    fn feasibility_rejects_a_brain_whose_default_can_be_gated_off() {
        // Force every consideration of every behaviour to a curve the world can switch off. `decide`
        // would then find no eligible rank bucket and fall through to behaviour 0 — the rank-4 role DUTY
        // — silently making the unit examine/heal/breach instead of standing down. Such a brain must
        // never enter the archive.
        for role in RoleId::ALL {
            let mut behaviors = default_behaviors_for_test(role);
            for behavior in &mut behaviors {
                for consideration in &mut behavior.considerations {
                    consideration.curve = gated(consideration.curve);
                }
            }
            let template = default_behaviors_for_test(role);
            let genome = encode(&behaviors);
            assert!(
                is_feasible(role, &template, &genome).is_err(),
                "{role:?}: a fully gated brain has no unconditional default and must be rejected"
            );
        }
    }

    #[test]
    fn an_all_zero_genome_is_feasible_because_a_flat_logistic_is_constant() {
        // A documented degenerate of THIS search space, pinned so it is never mistaken for a bug.
        //
        // Zeroing every parameter does NOT produce a dead brain: `Logistic { k: 0, x0: 0 }` evaluates to
        // 1/(1+e^0) = 0.5 for every input, which clears MIN_SCORE unconditionally. So the shared `flee()`
        // tail — the rank-6 top of the ladder — turns into a constant, and the squad flees forever while
        // passing every startup guard.
        //
        // The genome-level minimal criterion cannot catch this, and must not pretend to: a constant brain
        // is *loadable*, just not *interesting*. It is rejected later by the behavioural minimal criterion
        // ("a real encounter happened": a crab died, a unit was hurt, the map was explored), exactly as
        // POET admits an environment only when it is neither too easy nor too hard (Wang et al. §3).
        let template = default_behaviors_for_test(RoleId::Gunman);
        let zeroed = Genome { params: vec![0.0; encode(&template).params.len()], ranks: encode(&template).ranks };
        assert!(
            is_feasible(RoleId::Gunman, &template, &zeroed).is_ok(),
            "a flat Logistic is a constant 0.5, so this brain loads — the behavioural MC must reject it"
        );
        let decoded = decode(&template, &zeroed).expect("decode");
        let flee = decoded.iter().find(|b| b.rank == 6).expect("the rank-6 Flee tail");
        assert_eq!(flee.mode, crate::ai::utility::Mode::Flee);
    }
}
