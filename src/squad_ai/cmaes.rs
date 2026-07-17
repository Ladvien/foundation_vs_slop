//! **Separable CMA-ES** — the adaptive emitter behind the CMA-ME upgrade to the MAP-Elites search.
//!
//! The shipped search mutates with an isotropic Gaussian (`genome::flat_mutate`): a fixed, axis-aligned,
//! diagonal-identity step. CMA-ES (Hansen & Ostermeier 2001) instead *learns* the shape and scale of the
//! successful search direction — a per-coordinate step size and a global step length adapted from the
//! ranking of recent candidates — which is dramatically more sample-efficient on the correlated, ill-scaled
//! landscapes these genomes induce. Pairing it with MAP-Elites is **CMA-ME** (Fontaine et al. 2020,
//! "Covariance Matrix Adaptation for the Rapid Illumination of Behavior Space"); Ferigo et al. 2023
//! (DOI 10.1007/s00521-023-09124-5) show CMA-ME beating goal-directed search on exactly this kind of
//! policy/QD illumination.
//!
//! This is the **separable** variant (Ros & Hansen 2008): the covariance matrix is kept **diagonal**, so
//! there is no eigendecomposition — every update is O(n) and needs no linear-algebra dependency, which is
//! what makes it a clean pure-Rust fit. It gives up modelling coordinate *correlations* but keeps the two
//! things that matter here: per-coordinate scaling and cumulative step-size control (CSA).
//!
//! Determinism: every random draw goes through the seeded [`ChaCha8Rng`] (via `genome::gaussian`), never
//! entropy — the search stays bit-reproducible from its seed, exactly like the isotropic path.
//!
//! Reference equations follow Hansen, "The CMA Evolution Strategy: A Tutorial" (2016), with the sep-CMA
//! learning-rate scaling of the rank-one/rank-µ terms by `(n+2)/3`.

use rand_chacha::ChaCha8Rng;

use super::genome::gaussian;

/// One drawn candidate's bookkeeping, returned by [`SepCmaEs::ask`] and handed back in [`SepCmaEs::tell`].
/// Carries the standard-normal draw `z` and the scaled step `y = sqrt(C) .* z`, both needed for the update.
#[derive(Clone, Debug)]
pub struct Sample {
    /// The candidate point `x = mean + sigma * y` (the thing that gets evaluated), in `f32` for the genome.
    pub x: Vec<f32>,
    z: Vec<f64>,
    y: Vec<f64>,
}

/// A separable CMA-ES optimiser over an `n`-dimensional real vector. Maximises the fitness handed to
/// [`tell`](Self::tell) (candidates are ranked best-first by it).
pub struct SepCmaEs {
    n: usize,
    lambda: usize,
    mu: usize,
    weights: Vec<f64>,
    mu_eff: f64,
    c_sigma: f64,
    d_sigma: f64,
    c_c: f64,
    c_1: f64,
    c_mu: f64,
    chi_n: f64,

    mean: Vec<f64>,
    sigma: f64,
    /// Diagonal of the covariance matrix (per-coordinate variances).
    c: Vec<f64>,
    p_sigma: Vec<f64>,
    p_c: Vec<f64>,
    n_gen: u64,
}

impl SepCmaEs {
    /// New optimiser centred at `mean` with initial step size `sigma` and population size `lambda`
    /// (`lambda >= 4`; clamped up if smaller). Strategy parameters follow Hansen's tutorial, with the
    /// sep-CMA `(n+2)/3` scaling on the covariance learning rates.
    pub fn new(mean: Vec<f32>, sigma: f32, lambda: usize) -> Self {
        let n = mean.len().max(1);
        let lambda = lambda.max(4);
        let mu = lambda / 2;

        // Recombination weights (positive, log-decreasing), normalised to sum 1.
        let mut weights: Vec<f64> = (0..mu).map(|i| ((mu as f64) + 0.5).ln() - ((i as f64) + 1.0).ln()).collect();
        let w_sum: f64 = weights.iter().sum();
        for w in &mut weights {
            *w /= w_sum;
        }
        let mu_eff = 1.0 / weights.iter().map(|w| w * w).sum::<f64>();

        let nf = n as f64;
        let c_sigma = (mu_eff + 2.0) / (nf + mu_eff + 5.0);
        let d_sigma = 1.0 + 2.0 * (0.0f64).max(((mu_eff - 1.0) / (nf + 1.0)).sqrt() - 1.0) + c_sigma;
        let c_c = (4.0 + mu_eff / nf) / (nf + 4.0 + 2.0 * mu_eff / nf);
        let c_1_full = 2.0 / ((nf + 1.3).powi(2) + mu_eff);
        let c_mu_full =
            ((1.0 - c_1_full).min(2.0 * (mu_eff - 2.0 + 1.0 / mu_eff) / ((nf + 2.0).powi(2) + mu_eff))).max(0.0);
        // sep-CMA: scale the covariance learning rates up (a diagonal model can afford larger steps).
        let sep = ((nf + 2.0) / 3.0).max(1.0);
        let mut c_1 = c_1_full * sep;
        let mut c_mu = c_mu_full * sep;
        if c_1 + c_mu > 1.0 {
            let s = c_1 + c_mu;
            c_1 /= s;
            c_mu /= s;
        }
        let chi_n = nf.sqrt() * (1.0 - 1.0 / (4.0 * nf) + 1.0 / (21.0 * nf * nf));

        SepCmaEs {
            n,
            lambda,
            mu,
            weights,
            mu_eff,
            c_sigma,
            d_sigma,
            c_c,
            c_1,
            c_mu,
            chi_n,
            mean: mean.iter().map(|&x| x as f64).collect(),
            sigma: sigma.max(1e-6) as f64,
            c: vec![1.0; n],
            p_sigma: vec![0.0; n],
            p_c: vec![0.0; n],
            n_gen: 0,
        }
    }

    /// Population size (how many [`ask`](Self::ask) calls make one generation).
    pub fn lambda(&self) -> usize {
        self.lambda
    }

    /// The current global step size — a stagnation signal (a tiny sigma means the emitter has converged and
    /// should be restarted).
    pub fn sigma(&self) -> f32 {
        self.sigma as f32
    }

    /// Draw one candidate `x = mean + sigma * sqrt(C) .* z`, `z ~ N(0, I)`.
    pub fn ask(&self, rng: &mut ChaCha8Rng) -> Sample {
        let z: Vec<f64> = (0..self.n).map(|_| f64::from(gaussian(rng))).collect();
        let y: Vec<f64> = (0..self.n).map(|j| self.c[j].sqrt() * z[j]).collect();
        let x: Vec<f32> = (0..self.n).map(|j| (self.mean[j] + self.sigma * y[j]) as f32).collect();
        Sample { x, z, y }
    }

    /// Re-derive the `(z, y)` of an externally-adjusted point `x` — e.g. one a caller box-clamped back into
    /// the feasible region — so [`tell`](Self::tell) updates the distribution from the point **actually
    /// evaluated**, not the raw proposal. This is the standard "repair + re-inject" boundary handling that
    /// stops the mean marching past a clamp (Hansen's boundary-handling note). `x` must have length `n`;
    /// a shorter slice truncates to what the emitter tracks. `sigma > 0` and `C_j >= 1e-12`, so no div-by-0.
    pub fn repair(&self, x: &[f32]) -> Sample {
        let y: Vec<f64> = (0..self.n).map(|j| (f64::from(x[j.min(x.len().saturating_sub(1))]) - self.mean[j]) / self.sigma).collect();
        let z: Vec<f64> = (0..self.n).map(|j| y[j] / self.c[j].sqrt()).collect();
        Sample { x: x.to_vec(), z, y }
    }

    /// Update mean, covariance diagonal, and step size from a generation's evaluated candidates. `samples`
    /// is `(sample, fitness)` pairs; they are ranked **best-fitness-first** internally, so the caller need
    /// not sort. A short/empty generation (fewer than `mu` samples) is ignored (no update).
    pub fn tell(&mut self, mut samples: Vec<(Sample, f32)>) {
        if samples.len() < self.mu {
            return;
        }
        // Rank best-first (descending fitness — this is a maximiser).
        // SORT-OK: CMA-ES samples from its own seeded RNG, not an ECS query.
        samples.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        self.n_gen += 1;

        // Weighted recombination in z- and y-space over the best mu.
        let mut z_w = vec![0.0f64; self.n];
        let mut y_w = vec![0.0f64; self.n];
        for (i, w) in self.weights.iter().enumerate() {
            let (s, _) = &samples[i];
            for j in 0..self.n {
                z_w[j] += w * s.z[j];
                y_w[j] += w * s.y[j];
            }
        }

        // Move the mean: m <- m + sigma * y_w.
        for j in 0..self.n {
            self.mean[j] += self.sigma * y_w[j];
        }

        // Step-size path (CSA). For a diagonal C, C^{-1/2} y_w == z_w.
        let cs_factor = (self.c_sigma * (2.0 - self.c_sigma) * self.mu_eff).sqrt();
        for j in 0..self.n {
            self.p_sigma[j] = (1.0 - self.c_sigma) * self.p_sigma[j] + cs_factor * z_w[j];
        }
        let ps_norm = self.p_sigma.iter().map(|v| v * v).sum::<f64>().sqrt();
        self.sigma *= ((self.c_sigma / self.d_sigma) * (ps_norm / self.chi_n - 1.0)).exp();
        self.sigma = self.sigma.clamp(1e-9, 1e6);

        // Heaviside on the step-size path (stalls the rank-one update just after a big step).
        let denom = (1.0 - (1.0 - self.c_sigma).powi(2 * (self.n_gen as i32))).sqrt();
        let h_sigma = if ps_norm / denom.max(1e-12) < (1.4 + 2.0 / (self.n as f64 + 1.0)) * self.chi_n {
            1.0
        } else {
            0.0
        };

        // Anisotropic path and the diagonal covariance update.
        let cc_factor = (self.c_c * (2.0 - self.c_c) * self.mu_eff).sqrt();
        for j in 0..self.n {
            self.p_c[j] = (1.0 - self.c_c) * self.p_c[j] + h_sigma * cc_factor * y_w[j];
        }
        let delta_h = (1.0 - h_sigma) * self.c_c * (2.0 - self.c_c); // <= 1
        for j in 0..self.n {
            let rank_one = self.p_c[j] * self.p_c[j] + delta_h * self.c[j];
            let mut rank_mu = 0.0f64;
            for (i, w) in self.weights.iter().enumerate() {
                let (s, _) = &samples[i];
                rank_mu += w * s.y[j] * s.y[j];
            }
            self.c[j] = (1.0 - self.c_1 - self.c_mu) * self.c[j] + self.c_1 * rank_one + self.c_mu * rank_mu;
            // Guard against a coordinate variance collapsing to zero or drifting non-finite.
            if !self.c[j].is_finite() || self.c[j] < 1e-12 {
                self.c[j] = 1e-12;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::seeded;

    /// Sphere objective centred at `target`, as a maximiser: `-||x - target||²`.
    fn neg_sphere(x: &[f32], target: &[f32]) -> f32 {
        -x.iter().zip(target).map(|(a, b)| (a - b) * (a - b)).sum::<f32>()
    }

    #[test]
    fn converges_toward_the_optimum_and_shrinks_sigma() {
        // The canonical CMA-ES sanity check: on a sphere it must walk the mean to the optimum and contract
        // the step size. A broken update (wrong sign, bad path) fails one or both.
        let target = vec![1.5f32, -2.0, 0.5, 3.0];
        let mut es = SepCmaEs::new(vec![0.0; 4], 1.0, 12);
        let mut rng = seeded(0xC0A5_5EED);
        let start_err: f32 = neg_sphere(&es.mean.iter().map(|&v| v as f32).collect::<Vec<_>>(), &target);
        let start_sigma = es.sigma();
        for _ in 0..80 {
            let batch: Vec<_> = (0..es.lambda()).map(|_| es.ask(&mut rng)).collect();
            let scored: Vec<(Sample, f32)> = batch.into_iter().map(|s| { let f = neg_sphere(&s.x, &target); (s, f) }).collect();
            es.tell(scored);
        }
        let mean_f32: Vec<f32> = es.mean.iter().map(|&v| v as f32).collect();
        let end_err = neg_sphere(&mean_f32, &target);
        assert!(end_err > start_err, "fitness must improve: {end_err} !> {start_err}");
        assert!(end_err > -0.05, "the mean should get close to the optimum, err² = {}", -end_err);
        assert!(es.sigma() < start_sigma, "sigma must contract as it converges: {} !< {start_sigma}", es.sigma());
    }

    #[test]
    fn is_deterministic_from_the_seed() {
        let target = vec![0.3f32, 0.7];
        let run = || {
            let mut es = SepCmaEs::new(vec![0.0; 2], 0.5, 8);
            let mut rng = seeded(42);
            for _ in 0..15 {
                let scored: Vec<(Sample, f32)> =
                    (0..es.lambda()).map(|_| es.ask(&mut rng)).map(|s| { let f = neg_sphere(&s.x, &target); (s, f) }).collect();
                es.tell(scored);
            }
            es.mean.clone()
        };
        assert_eq!(run(), run(), "same seed must reproduce the same trajectory");
    }

    #[test]
    fn a_short_generation_is_ignored() {
        let mut es = SepCmaEs::new(vec![0.0; 3], 1.0, 8);
        let before = es.mean.clone();
        es.tell(vec![]); // nothing to recombine
        assert_eq!(es.mean, before, "an empty tell must not move the mean");
    }
}
