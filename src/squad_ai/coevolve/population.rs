//! The `Population<G>` container: the elite archive plus the sampling the emitters draw from.
//! Split out of the former single-file `coevolve.rs`; a pure move (FVS-N-3).

use super::*;

// ── Populations ──────────────────────────────────────────────────────────────────────────────────

/// A MAP-Elites archive plus the genomes its elites refer to. `qd::Elite` stores an opaque `u64` handle;
/// here that handle is an index into `store`, so the archive math stays exactly as unit-tested.
pub struct Population<G> {
    pub archive: MapElitesArchive,
    store: Vec<G>,
}

impl<G: Clone> Population<G> {
    pub fn new(resolution: usize) -> Self {
        Population { archive: MapElitesArchive::new(resolution), store: Vec::new() }
    }

    /// Insert if this genome fills an empty niche or beats the incumbent. Returns whether it landed.
    ///
    /// The genome is stored **only on acceptance**. A rejected candidate's handle was never written into
    /// any `Elite`, so keeping its slot would leak one genome per proposal — over 98% of `store` on a real
    /// run. Accepted handles are still never recycled, so an incumbent's handle stays valid when it is
    /// later displaced.
    pub fn insert(&mut self, descriptor: BehaviorDescriptor, fitness: f32, genome: G) -> bool {
        let handle = self.store.len() as u64;
        if self.archive.insert(descriptor, fitness, handle) {
            self.store.push(genome);
            true
        } else {
            false
        }
    }

    /// Insert a challenger, resolving a contested cell by a **common-opponent** comparison instead of the
    /// archive's stored fitness — the fix for MAP-Elites' stationary-fitness assumption (Mouret & Clune
    /// rest their argument on a stationary `f(genome)`; ours is not — `W`, `L`, and the descriptor all
    /// depend on the opponents a candidate drew). Comparing a challenger's fitness to an incumbent's scored
    /// against *different* opponents is apples-to-oranges, so when the cell is occupied `reeval_incumbent`
    /// re-scores the incumbent against the challenger's **exact** opponents and seeds (POET's
    /// `EVALUATE_CANDIDATES`, arXiv:1901.01753); the challenger wins unless the incumbent still scores `>=`
    /// it under those identical conditions. On a hold the incumbent's stored fitness is refreshed to the
    /// fresh common-opponent value, so the cell stays comparable going forward.
    ///
    /// `reeval_incumbent` returns `None` when the incumbent produces no real encounter on any of the
    /// challenger's conditions (inadmissible there) — then the challenger, which did, wins. It consumes no
    /// fresh RNG (it replays recorded seeds), so the whole run stays reproducible from `cfg.seed`.
    pub fn try_insert_with_reeval(
        &mut self,
        descriptor: BehaviorDescriptor,
        challenger_fitness: f32,
        challenger: G,
        reeval_incumbent: impl FnOnce(&G) -> Result<Option<f32>, String>,
    ) -> Result<bool, String> {
        match self.archive.incumbent(descriptor) {
            None => {
                let handle = self.store.len() as u64;
                self.store.push(challenger);
                self.archive.place(descriptor, challenger_fitness, handle);
                Ok(true)
            }
            Some(inc) => {
                let incumbent_genome = self.get(inc.genome).ok_or("dangling elite handle")?.clone();
                match reeval_incumbent(&incumbent_genome)? {
                    // Incumbent holds under the common opponents; refresh its fitness to the fresh value.
                    Some(s) if s >= challenger_fitness => {
                        self.archive.place(inc.descriptor, s, inc.genome);
                        Ok(false)
                    }
                    // Incumbent inadmissible under these conditions (`None`) or worse: the challenger wins.
                    _ => {
                        let handle = self.store.len() as u64;
                        self.store.push(challenger);
                        self.archive.place(descriptor, challenger_fitness, handle);
                        Ok(true)
                    }
                }
            }
        }
    }

    pub fn get(&self, handle: u64) -> Option<&G> {
        self.store.get(handle as usize)
    }

    /// Uniform draw from the occupied niches — the MAP-Elites selection rule. Uniform over *cells*, not
    /// over fitness: that is what keeps the search expanding into empty regions of the behaviour space
    /// rather than piling onto the current best.
    pub fn sample_parent(&self, rng: &mut ChaCha8Rng) -> Option<&G> {
        if self.archive.is_empty() {
            return None;
        }
        let n = self.archive.iter().count();
        let pick = rng.below(n);
        let handle = self.archive.iter().nth(pick).map(|(_, e)| e.genome)?;
        self.get(handle)
    }

    /// `k` opponents drawn (with replacement) from across the archive. Sampling the whole archive rather
    /// than its incumbent is the anti-cycling rule; with replacement so a sparse archive still yields `k`.
    pub fn sample_opponents(&self, k: usize, rng: &mut ChaCha8Rng) -> Vec<&G> {
        (0..k).filter_map(|_| self.sample_parent(rng)).collect()
    }
}
