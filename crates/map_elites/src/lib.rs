//! **Quality-Diversity, as a library.**
//!
//! MAP-Elites illuminates the space of behaviours a parameterised system can produce, rather than
//! optimising toward one best point: the archive keeps the fittest genome found *per behaviour niche*,
//! so what comes back is a map of what is achievable and how, not a single winner.
//!
//! References:
//! - Mouret & Clune, "Illuminating search spaces by mapping elites", arXiv:1504.04909 (2015) — the
//!   archive, the niche grid, and the uniform-over-occupied-cells selection rule.
//! - Pugh, Soros & Clune, "Quality Diversity: A New Frontier for Evolutionary Computation",
//!   Frontiers in Robotics and AI (2016) — the framing, and why QD beats objective-only search on
//!   deceptive landscapes.
//! - Fontaine et al., "Covariance Matrix Adaptation for the Rapid Illumination of Behavior Space",
//!   GECCO 2020 — CMA-ME, the emitter in [`loops`] that drives [`cmaes`].
//! - Fontaine & Nikolaidis, "Differentiable Quality Diversity", NeurIPS 2021 — CMA-MAE's archive
//!   learning rate.
//! - Wang et al., "Paired Open-Ended Trailblazer (POET)", arXiv:1901.01753 (2019) — the outer loop in
//!   [`poet`] that co-evolves environments with the agents solving them.
//!
//! # What this crate is not
//!
//! It has no evaluator. Every loop here is parameterised over a genome type `G` and takes the
//! mutation and the evaluation as closures, because "how good is this genome" is the one question
//! only the caller can answer. That is what keeps the crate engine-free.
//!
//! # Reproducibility is the point
//!
//! Every draw goes through a seeded `ChaCha8Rng` and every archive iteration is over a `BTreeMap`, so
//! a run replays bit-for-bit from one `u64` on any platform. `tests/engine_free.rs` fails the build if
//! a dependency arrives that could put a renderer, a thread pool, or an entropy source in that path.

pub mod cmaes;
pub mod experience;
pub mod fairness;
pub mod interest;
pub mod loops;
pub mod poet;
pub mod population;
pub mod qd;
pub mod replayability;

use emerge_core::rng::DetRng;
use rand_chacha::ChaCha8Rng;

/// A standard normal draw (Box–Muller). `unit()` yields `[0, 1)`, so `1.0 - unit()` moves it to
/// `(0, 1]` and keeps `ln` finite.
///
/// **One Gaussian kernel for the whole search stack.** It lives at the crate root rather than beside
/// any one genome because [`cmaes`] and every flat-vector genome mutator draw from it in a
/// load-bearing ORDER — a second implementation would not fail a test, it would quietly make two
/// searches irreproducible against each other.
pub fn gaussian(rng: &mut ChaCha8Rng) -> f32 {
    let u1 = 1.0 - rng.unit();
    let u2 = rng.unit();
    let r = (-2.0 * u1.ln()).sqrt();
    (r * (std::f64::consts::TAU * u2).cos()) as f32
}
