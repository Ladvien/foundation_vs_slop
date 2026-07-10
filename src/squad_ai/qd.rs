//! **Quality-Diversity** harness — the "evolve into an interesting game" loop. Rather than optimise one
//! best squad, MAP-Elites *illuminates* the space of squad behaviours: it keeps the highest-performing
//! configuration found in each cell of a behaviour-descriptor grid, so a designer can browse the whole
//! range of playstyles a role tuning produces and pick the interesting ones (Mouret & Clune,
//! "Illuminating search spaces by mapping elites", 2015; Pugh, Soros & Stanley, "Quality Diversity: A
//! New Frontier for Evolutionary Computation", 2016).
//!
//! An episode is run headless on the deterministic core (repeatable), summarised into a
//! [`BehaviorDescriptor`] + a fitness (an interestingness proxy grounded in engagement factors —
//! challenge, variety, cohesion — after the GUESS scale / self-determination theory), and inserted
//! into the [`MapElitesArchive`]. This module is the pure archive + descriptor math; wiring it to the
//! headless `sim_harness` is the training entry point.

use std::collections::BTreeMap;

/// A 2-D behaviour characterisation of one squad configuration, each axis in `[0,1]`:
/// - `aggression`: how much the squad engages threats vs. avoids (combat share of actions).
/// - `exploration`: how much of the map the squad covered (visitation coverage / reachable area).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BehaviorDescriptor {
    pub aggression: f32,
    pub exploration: f32,
}

impl BehaviorDescriptor {
    pub fn new(aggression: f32, exploration: f32) -> Self {
        BehaviorDescriptor {
            aggression: aggression.clamp(0.0, 1.0),
            exploration: exploration.clamp(0.0, 1.0),
        }
    }

    /// Bin to a grid cell at the given resolution (cells per axis).
    fn cell(&self, res: usize) -> (usize, usize) {
        let bin = |x: f32| ((x * res as f32) as usize).min(res.saturating_sub(1));
        (bin(self.aggression), bin(self.exploration))
    }
}

/// One occupant of an archive cell: the best configuration found for that behaviour niche.
#[derive(Clone, Copy, Debug)]
pub struct Elite {
    pub descriptor: BehaviorDescriptor,
    pub fitness: f32,
    /// An opaque handle to the configuration that produced it (e.g. a seed / genome id).
    pub genome: u64,
}

/// The MAP-Elites feature grid: at most one elite per behaviour cell, always the highest fitness seen
/// (Mouret & Clune 2015). A **`BTreeMap`** (not `HashMap`) so iteration is in a fixed, sorted cell
/// order: `std::collections::HashMap`'s per-process-randomized order would make `best()`'s tie-break
/// and `qd_score()`'s non-associative f32 sum differ run-to-run, breaking the module's documented
/// "run headless on the deterministic core (repeatable)" guarantee.
pub struct MapElitesArchive {
    res: usize,
    cells: BTreeMap<(usize, usize), Elite>,
}

impl MapElitesArchive {
    /// A `res × res` archive (e.g. 16 → 256 behaviour niches).
    pub fn new(res: usize) -> Self {
        MapElitesArchive {
            res: res.max(1),
            cells: BTreeMap::new(),
        }
    }

    /// Try to place a configuration. Returns `true` if it filled an empty niche or beat the incumbent
    /// (the MAP-Elites elitism rule); `false` if a better elite already holds the cell.
    pub fn insert(&mut self, descriptor: BehaviorDescriptor, fitness: f32, genome: u64) -> bool {
        let key = descriptor.cell(self.res);
        match self.cells.get(&key) {
            Some(e) if e.fitness >= fitness => false,
            _ => {
                self.cells.insert(key, Elite { descriptor, fitness, genome });
                true
            }
        }
    }

    /// The elite currently holding `descriptor`'s cell, if any. `Elite` is `Copy`, so this hands back a
    /// value — letting the search re-evaluate that incumbent under a challenger's conditions before the
    /// (non-stationary) elitism test, without holding a borrow on the archive (POET's `EVALUATE_CANDIDATES`
    /// common-opponent comparison; Wang et al., arXiv:1901.01753).
    pub fn incumbent(&self, descriptor: BehaviorDescriptor) -> Option<Elite> {
        self.cells.get(&descriptor.cell(self.res)).copied()
    }

    /// Place an elite **unconditionally**, bypassing `insert`'s elitism test. Used only after the search
    /// loop has already decided the winner by a common-opponent comparison (see
    /// `coevolve::Population::try_insert_with_reeval`); this just commits it into the cell.
    pub fn place(&mut self, descriptor: BehaviorDescriptor, fitness: f32, genome: u64) {
        self.cells.insert(descriptor.cell(self.res), Elite { descriptor, fitness, genome });
    }

    /// How many behaviour niches are occupied — the QD "coverage" metric (breadth of playstyles found).
    pub fn coverage(&self) -> usize {
        self.cells.len()
    }

    /// Every occupied niche, in the archive's fixed sorted cell order (the `BTreeMap` is deliberate —
    /// see the type docs). Used to sample a MAP-Elites parent and to sample coevolutionary opponents.
    pub fn iter(&self) -> impl Iterator<Item = (&(usize, usize), &Elite)> {
        self.cells.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The archive resolution (cells per axis).
    pub fn resolution(&self) -> usize {
        self.res
    }

    /// The single highest-fitness elite across the whole archive.
    pub fn best(&self) -> Option<&Elite> {
        self.cells.values().max_by(|a, b| a.fitness.total_cmp(&b.fitness))
    }

    /// Sum of every cell's fitness — the QD-score (quality × diversity in one number).
    pub fn qd_score(&self) -> f32 {
        self.cells.values().map(|e| e.fitness).sum()
    }
}

/// Summary statistics from one headless episode, from which the descriptor + interestingness fitness
/// are computed.
#[derive(Clone, Copy, Debug)]
pub struct EpisodeStats {
    /// Fraction of unit-decisions that were combat actions (Overwatch/Engage/Suppress).
    pub combat_share: f32,
    /// Distinct cells the squad occupied.
    pub cells_covered: u32,
    /// Reachable floor cells (denominator for exploration).
    pub reachable_cells: u32,
    /// Mean distance of a unit from the squad anchor (cohesion tightness; smaller = tighter).
    pub mean_spread: f32,
    /// Distinct action modes used across the episode (behavioural variety).
    pub distinct_modes: u32,
    /// Units still alive at episode end (survival).
    pub survivors: u32,
    pub squad_size: u32,
}

impl EpisodeStats {
    pub fn descriptor(&self) -> BehaviorDescriptor {
        let exploration = if self.reachable_cells > 0 {
            self.cells_covered as f32 / self.reachable_cells as f32
        } else {
            0.0
        };
        BehaviorDescriptor::new(self.combat_share, exploration)
    }

    /// Interestingness fitness `[0,1]`: reward behavioural variety, survival, and *moderate* cohesion
    /// (a squad that neither clumps into one tile nor scatters uselessly), after engagement research
    /// (GUESS: challenge + variety; SDT: competence/relatedness). This is the quality axis QD improves
    /// per niche; the *diversity* is handled by the archive grid, so fitness need not encode it.
    pub fn fitness(&self) -> f32 {
        let variety = (self.distinct_modes as f32 / 8.0).clamp(0.0, 1.0);
        let survival = if self.squad_size > 0 {
            self.survivors as f32 / self.squad_size as f32
        } else {
            0.0
        };
        // Cohesion sweet-spot: peaks when mean spread ≈ 4 units, falls off if too tight or too loose.
        let cohesion = (1.0 - ((self.mean_spread - 4.0).abs() / 6.0)).clamp(0.0, 1.0);
        (0.4 * variety + 0.4 * survival + 0.2 * cohesion).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_keeps_best_per_cell_and_separates_niches() {
        let mut a = MapElitesArchive::new(16);
        let d = BehaviorDescriptor::new(0.5, 0.5);
        assert!(a.insert(d, 0.3, 1)); // empty niche → filled
        assert!(!a.insert(d, 0.2, 2)); // worse → rejected
        assert!(a.insert(d, 0.6, 3)); // better → replaces
        assert_eq!(a.coverage(), 1);
        assert_eq!(a.best().map(|e| e.genome), Some(3));
        // A different behaviour lands in its own niche.
        assert!(a.insert(BehaviorDescriptor::new(0.9, 0.1), 0.1, 4));
        assert_eq!(a.coverage(), 2);
    }

    #[test]
    fn descriptor_and_fitness_are_in_range() {
        let stats = EpisodeStats {
            combat_share: 0.6,
            cells_covered: 40,
            reachable_cells: 100,
            mean_spread: 4.0,
            distinct_modes: 6,
            survivors: 4,
            squad_size: 5,
        };
        let d = stats.descriptor();
        assert!((d.exploration - 0.4).abs() < 1e-6);
        assert_eq!(d.aggression, 0.6);
        let f = stats.fitness();
        assert!((0.0..=1.0).contains(&f));
        // Full-cohesion, high-variety, high-survival should score well.
        assert!(f > 0.5);
    }

    #[test]
    fn qd_score_sums_cell_fitness() {
        let mut a = MapElitesArchive::new(8);
        a.insert(BehaviorDescriptor::new(0.1, 0.1), 0.4, 1);
        a.insert(BehaviorDescriptor::new(0.9, 0.9), 0.5, 2);
        assert!((a.qd_score() - 0.9).abs() < 1e-6);
    }
}
