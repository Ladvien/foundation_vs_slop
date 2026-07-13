//! The **policy genome**: a learned squad controller's MLP weights viewed as a flat parameter vector — the
//! sixth evolvable population, and the one that fills the RL gap.
//!
//! Where [`super::behavior_genome`] / [`super::world_genome`] evolve *config dials* the hand-authored brain
//! reads, this evolves the **decision layer itself**: the weights of a [`NeuralPolicy`] that replaces the
//! dual-utility `UtilityPolicy` at the squad policy seam (`super::policy`). It is **neuroevolution** —
//! Evolution Strategies over policy weights (Salimans et al. 2017, "Evolution Strategies as a Scalable
//! Alternative to Reinforcement Learning", arXiv:1703.03864) — chosen over gradient PPO because it is
//! gradient-free (no autodiff in Rust), embarrassingly parallel, and slots into the existing MAP-Elites /
//! co-evolution engine with no new evaluation machinery: a genome is a weight vector, evaluated by the same
//! [`super::evaluate::rollout_with_policy`] and scored by the same witnessed-learnable-surprise fitness.
//!
//! Mirrors the flat-genome pattern of `world_genome`: a `Vec<f32>` newtype + a uniform [`BOUNDS`] table +
//! `flat_mutate` / `flat_range_check`. Unlike the config genomes, the bounds are **uniform** (`[-W, W]` per
//! weight) rather than per-knob physical ranges — a weight has no semantic unit, only a magnitude the
//! `tanh` hidden layer keeps well-conditioned. Feasibility is therefore just "finite and in `[-W, W]`";
//! there is no `validate_tuning` analogue because a weight vector cannot be *semantically* invalid.
//!
//! One caveat, stated honestly: unlike a `behavior:` config diff, an MLP weight vector is **not
//! human-readable**, so the "readable elite = reward-hacking guard" property the config genomes rely on
//! (Skalse et al., arXiv:2209.13085) does not hold here. The guard for a learned policy is instead
//! behavioural: the minimal criterion + watching it play (`train rl` → boot the game with the elite).

use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::genome::{flat_mutate, flat_range_check, gaussian};
use super::policy::NeuralPolicy;

/// Symmetric per-weight bound. Weights initialise and mutate within `[-W, W]`; a few units of range spans
/// `tanh` saturation either way, so the net can express hard switches without the search wandering off to
/// unbounded magnitudes.
const W: f32 = 4.0;

/// Number of searched weights = the MLP's parameter count (`NeuralPolicy::WEIGHT_COUNT`).
pub const N: usize = NeuralPolicy::WEIGHT_COUNT;

/// Uniform bounds — every weight shares `[-W, W]` (unlike the config genomes' per-knob physical ranges).
static BOUNDS: [(f32, f32); N] = [(-W, W); N];

/// The seed used to draw the [`authored`] band origin. A fixed constant (no entropy) so the search is
/// reproducible; the value is arbitrary (the golden-ratio odd constant, a conventional splitmix seed).
const AUTHORED_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// The standard deviation of the [`authored`] band origin's weights. Small but nonzero: a *zero* origin
/// would collapse every `flat_mutate` step to the scale floor and the search could never build up
/// magnitude, while a large origin would start the seed policy in `tanh` saturation.
const AUTHORED_SIGMA: f32 = 0.5;

/// A learned policy's weights, flattened. Meaningless without [`decode`], which slices it into the MLP.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolicyGenome(pub Vec<f32>);

/// The seed / baseline genome — a fixed pseudo-random small-weight net, the band origin for [`mutate`] and
/// the archive-seeding candidate. Deterministic (seeded from a constant, no entropy) so the search is
/// reproducible; nonzero so `flat_mutate`'s scale-relative kick can actually explore. Every weight is
/// clamped into [`BOUNDS`], so the result is feasible by construction.
pub fn authored() -> PolicyGenome {
    let mut rng = crate::rng::seeded(AUTHORED_SEED);
    let v: Vec<f32> = (0..N)
        .map(|i| (gaussian(&mut rng) * AUTHORED_SIGMA).clamp(BOUNDS[i].0, BOUNDS[i].1))
        .collect();
    PolicyGenome(v)
}

/// Rebuild a [`NeuralPolicy`] from the genome's weight vector. `Err` on wrong length — one path, no
/// padding/truncation (delegates to `NeuralPolicy::from_weights`, which pins the layer layout).
pub fn decode(g: &PolicyGenome) -> Result<NeuralPolicy, String> {
    if g.0.len() != N {
        return Err(format!("policy genome has {} weights, expected {N}", g.0.len()));
    }
    NeuralPolicy::from_weights(&g.0)
}

/// Perturb a policy genome: every weight gets a scale-relative Gaussian kick (scale from the **authored**
/// origin so bands can't ratchet across generations), clamped to `[-W, W]`. Children are feasible by
/// construction — one `flat_mutate`, no rejection loop. Reuses `genome::flat_mutate` (shared with every
/// other flat genome).
pub fn mutate(
    parent: &PolicyGenome,
    authored: &PolicyGenome,
    sigma: f32,
    rng: &mut ChaCha8Rng,
) -> Result<PolicyGenome, String> {
    if parent.0.len() != N {
        return Err(format!("policy genome has {} weights, expected {N}", parent.0.len()));
    }
    if authored.0.len() != N {
        return Err(format!("authored policy genome has {} weights, expected {N}", authored.0.len()));
    }
    Ok(PolicyGenome(flat_mutate(&parent.0, &authored.0, &BOUNDS, sigma, rng)))
}

/// Build a genome from a raw real vector by clamping each weight into `[-W, W]` — the `from_vec` projection
/// the CMA-ME emitter (`map_elites::map_elites_cma_loop`) uses to pull an unconstrained CMA proposal back
/// into the feasible box. Deliberately does **not** pad or truncate to `N`: it preserves the input length, so
/// a wrong-length vector yields a wrong-length genome that [`is_feasible`] / [`decode`] reject loudly at the
/// door (one path, no silent zero-fill). `BOUNDS` is uniform, so clamping to the scalar bound is exact.
pub fn from_vec_clamped(v: &[f32]) -> PolicyGenome {
    PolicyGenome(v.iter().map(|&x| x.clamp(-W, W)).collect())
}

/// The feasibility gate: right length, every weight finite and within [`BOUNDS`]. `mutate` guarantees this
/// by construction; the check exists for genomes built any other way (e.g. loaded from a committed archive).
pub fn is_feasible(g: &PolicyGenome) -> Result<(), String> {
    if g.0.len() != N {
        return Err(format!("policy genome has {} weights, expected {N}", g.0.len()));
    }
    flat_range_check(&g.0, &BOUNDS, "policy")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::seeded;

    #[test]
    fn authored_is_feasible_and_decodes() {
        let g = authored();
        assert_eq!(g.0.len(), N);
        is_feasible(&g).expect("the seed policy must be feasible");
        decode(&g).expect("the seed policy must decode into an MLP");
    }

    #[test]
    fn authored_is_deterministic() {
        // No entropy: two calls must produce byte-identical weights, or the search is not reproducible.
        assert_eq!(authored(), authored());
    }

    #[test]
    fn mutation_stays_within_bounds_and_feasible() {
        let authored_g = authored();
        let mut rng = seeded(0x5EED);
        for _ in 0..200 {
            let child = mutate(&authored_g, &authored_g, 0.3, &mut rng).expect("mutate");
            assert_eq!(child.0.len(), N);
            for (i, &x) in child.0.iter().enumerate() {
                let (lo, hi) = BOUNDS[i];
                assert!(x.is_finite(), "weight {i} became non-finite");
                assert!((lo..=hi).contains(&x), "weight {i} = {x} escaped BOUNDS [{lo}, {hi}]");
            }
            is_feasible(&child).expect("a clamped child must be feasible by construction");
        }
    }

    #[test]
    fn a_mutation_actually_moves_something() {
        let authored_g = authored();
        let mut rng = seeded(0xC0DE);
        let child = mutate(&authored_g, &authored_g, 0.3, &mut rng).expect("mutate");
        assert_ne!(child, authored_g, "mutation changed nothing");
    }

    #[test]
    fn from_vec_clamps_into_bounds_without_padding() {
        // Clamps each entry into [-W, W], and PRESERVES length — no silent zero-fill. A wrong-length input
        // therefore yields a wrong-length genome that the feasibility gate rejects at the door.
        assert_eq!(from_vec_clamped(&[100.0, -100.0, 0.5]).0, vec![W, -W, 0.5]);
        assert_eq!(from_vec_clamped(&vec![0.0; N]).0.len(), N);
        assert!(is_feasible(&from_vec_clamped(&vec![0.0; N - 2])).is_err(), "short input must not be padded");
    }

    #[test]
    fn feasibility_rejects_wrong_length_and_out_of_range() {
        assert!(is_feasible(&PolicyGenome(vec![0.0; N - 1])).is_err());
        assert!(decode(&PolicyGenome(vec![0.0; N + 1])).is_err());
        let mut bad = authored();
        bad.0[0] = W + 1.0; // out of BOUNDS
        assert!(is_feasible(&bad).is_err());
    }
}
