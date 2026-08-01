//! **Where the anomalies go** — one level-wide placement pass, shared by every species.
//!
//! # The bug this replaces
//!
//! Five species each ran their own copy of the same greedy raster scan — `enemy::spawn_enemies`,
//! `scp999::spawn_scp999`, `scp1048::seed_bears`, `crab::setup::spawn_crabs`, and (per-room, but with
//! the same tail) `scp610::spawn_scp610_blooms`. Each scanned `for y { for x { .. } }` from cell (0,0)
//! and took the first cells that were far enough from the squad's entry.
//!
//! Two things fall out of that, and the player hit both at once (2026-08-01: *"610, 1048, and 1048-A,
//! and Smiley are all bundled in the corner. We should have rules about what spawns where."*):
//!
//! 1. **Raster order makes "far from spawn" mean "in the corner".** `Dungeon::spawn` is the site
//!    nearest the level *centre*, so the first row-major cell past a radius is always at low x / low y.
//!    A minimum distance consumed in scan order is a corner-seeking rule in disguise. (`broadcast.rs`
//!    documents the same trap costing that anomaly its whole feature — FVS-N-30.)
//! 2. **Nobody knew about anybody else.** Each scan tracked separation only from its *own* kind, so
//!    five species independently picked the same corner and stacked there. There was no cross-species
//!    spacing constraint anywhere in the codebase.
//!
//! # The rule now
//!
//! One pass places every anomaly in the level against a single shared set of already-placed sites, so
//! separation is **cross-species by construction** rather than per-species by accident. This is the
//! object-level half of what Smelik, Tutenel, de Kraker & Bidarra call *consistency maintenance*
//! ("A declarative approach to procedural modeling of virtual worlds", C&G 2010,
//! doi 10.1016/j.cag.2010.11.011): structure comes from *"connections … and constraints (e.g. minimum
//! distance between certain objects) between semantic objects"*, resolved centrally instead of by
//! whichever generator happened to run last.
//!
//! Site selection is **Mitchell's best-candidate** (Mitchell, "Spectrally optimal sampling for
//! computer graphics", SIGGRAPH 1991): draw [`CANDIDATE_TRIES`] seeded candidates from the eligible
//! set and keep whichever maximises the distance to the nearest already-placed anomaly. That is the
//! cheap approximation to Poisson-disk sampling (Bridson, "Fast Poisson Disk Sampling in Arbitrary
//! Dimensions", SIGGRAPH sketches 2007) and it spreads by *construction*: there is no scan order left
//! for a corner to win.
//!
//! The vocabulary is the placement IR's, per Stage B of
//! `slop/research/2026-07-24-world-population-grammar.md` — a species' entry is a
//! [`Predicate::Count`] over the level plus a [`Predicate::MinDistance`] against every other anomaly.
//!
//! # Determinism
//!
//! The pass draws from its **own** RNG sub-stream (`PLACEMENT_SEED ^ splitmix64(ANOMALY_STREAM)`), so
//! it cannot shift a single furniture draw and the furniture goldens are untouched by construction —
//! the same discipline `placement::mod` uses to keep regions independent. The eligible-cell list is
//! built by a raster walk over the grid and then `sort_total!`-keyed on the cell coordinate, which is
//! unique, so the input order is a function of the dungeon alone and never of an ECS query. Species
//! are visited in a fixed declared order, not in query or hash order.

use bevy::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::HashMap;

use crate::dungeon::Dungeon;
use crate::rng::DetRng;

/// Sub-stream tag, mixed into [`super::PLACEMENT_SEED`] so anomaly placement draws are disjoint from
/// every region's furniture stream. Changing this re-rolls anomaly sites and nothing else.
const ANOMALY_STREAM: u64 = 0xA_1101;

/// Candidates drawn per site for the best-candidate pass. Mitchell 1991 uses a count proportional to
/// the number already placed; a flat value is enough here because the counts are single digits, and a
/// fixed budget keeps the cost a predictable `sites × CANDIDATE_TRIES` rather than quadratic.
const CANDIDATE_TRIES: usize = 64;

/// One species' placement rule. Compiled from the species' **existing** config (its `count` and its
/// `spawn_min_dist`) rather than a new parallel config block — two sources of truth for "how many
/// bears" is exactly the drift this repo keeps paying for.
#[derive(Clone, Debug)]
pub struct AnomalyRule {
    /// Stable species key. Also the lookup key in [`AnomalySites`], and the order-defining sort key.
    pub key: &'static str,
    /// How many sites to place (`Predicate::Count` over the level).
    pub count: usize,
    /// Minimum Euclidean distance in tiles from the squad's entry cell.
    pub min_from_spawn: f32,
}

/// The solved sites, keyed by species. Written once per run in [`super::super::session::RunBuild::Grids`]
/// — i.e. after the `Dungeon` exists and before anything in `Populate` spawns — and read by each
/// species' spawner instead of it running its own scan.
///
/// A species with no entry got no sites, which is a **content fact worth seeing**: [`solve_anomaly_sites`]
/// warns loudly rather than quietly placing fewer. There is no fallback scan; if this table is short the
/// level genuinely could not hold the authored population under the separation rule.
#[derive(Resource, Default, Debug)]
pub struct AnomalySites(HashMap<&'static str, Vec<IVec2>>);

impl AnomalySites {
    /// The sites solved for a species, or an empty slice if it got none.
    pub fn get(&self, key: &str) -> &[IVec2] {
        self.0.get(key).map_or(&[], |v| v.as_slice())
    }

    /// Total placed sites across every species — the denominator for the dispersion metric and a
    /// convenient assertion target in tests.
    pub fn total(&self) -> usize {
        self.0.values().map(Vec::len).sum()
    }

    /// Every placed site, in a stable species-then-index order. Used by the level descriptor to measure
    /// how spread the population actually came out.
    pub fn all_sites(&self) -> Vec<IVec2> {
        let mut keys: Vec<&&'static str> = self.0.keys().collect();
        // SORT-OK: species keys are unique string literals; the map is keyed by them.
        keys.sort_unstable();
        keys.into_iter().flat_map(|k| self.0[k].iter().copied()).collect()
    }
}

/// Place every species' sites in one pass, honouring a shared cross-species `separation` (tiles).
///
/// **Takes the floor cells as plain data, not a `Dungeon`.** The interesting logic here is the
/// cross-species spacing and the candidate selection, neither of which needs to know what a dungeon is
/// — so this stays testable with a hand-built cell list, no `App`, no GPU, in the same spirit as
/// `ir.rs` keeping the whole IR engine-free. [`build_anomaly_sites`] is the thin Bevy edge that reads
/// the `Dungeon` and calls this.
///
/// `floor` must be a total-ordered list of floor cells (see [`floor_cells`]).
///
/// Returns the table plus, for each species that could not be fully placed, `(key, placed, wanted)` so
/// the caller can warn with specifics. Never panics and never substitutes a degraded placement.
pub fn solve_anomaly_sites(
    floor: &[IVec2],
    spawn: IVec2,
    rules: &[AnomalyRule],
    separation: f32,
    rng: &mut ChaCha8Rng,
) -> (AnomalySites, Vec<(&'static str, usize, usize)>) {
    let mut sites: HashMap<&'static str, Vec<IVec2>> = HashMap::new();
    // Every anomaly placed SO FAR, across all species — this shared list is the whole fix.
    let mut placed: Vec<IVec2> = Vec::new();
    let mut short: Vec<(&'static str, usize, usize)> = Vec::new();

    for rule in rules {
        let eligible: Vec<IVec2> = floor
            .iter()
            .copied()
            .filter(|c| (*c - spawn).as_vec2().length() >= rule.min_from_spawn)
            .collect();

        let mut mine: Vec<IVec2> = Vec::new();
        for _ in 0..rule.count {
            match best_candidate(&eligible, &placed, separation, rng) {
                Some(cell) => {
                    placed.push(cell);
                    mine.push(cell);
                }
                None => break,
            }
        }
        if mine.len() < rule.count {
            short.push((rule.key, mine.len(), rule.count));
        }
        sites.insert(rule.key, mine);
    }

    (AnomalySites(sites), short)
}

/// Mitchell's best-candidate: draw [`CANDIDATE_TRIES`] seeded samples from `eligible` and keep the one
/// whose nearest already-`placed` neighbour is farthest away, subject to the hard `separation` floor.
///
/// `None` means no sampled candidate cleared `separation`. That is reported as a shortfall rather than
/// relaxed: a rule that silently gives up its spacing is how five species ended up in one corner.
///
/// Sampling rather than scanning is the point. An exhaustive "farthest eligible cell" would be
/// deterministic too, but it is also *degenerate* — it drives every anomaly to the map's extremities,
/// which is the corner bug wearing the opposite sign. Best-candidate keeps the spread stochastic within
/// the seeded stream, which is what makes two seeds produce genuinely different populations.
fn best_candidate(
    eligible: &[IVec2],
    placed: &[IVec2],
    separation: f32,
    rng: &mut ChaCha8Rng,
) -> Option<IVec2> {
    if eligible.is_empty() {
        return None;
    }
    let mut best: Option<(u32, IVec2)> = None;
    for _ in 0..CANDIDATE_TRIES {
        let cell = eligible[rng.below(eligible.len())];
        let nearest = placed
            .iter()
            .map(|p| (*p - cell).as_vec2().length())
            .fold(f32::INFINITY, f32::min);
        if nearest < separation {
            continue; // violates the hard cross-species floor
        }
        // Compare on the bit pattern so the choice is exact-float and order-independent; break ties on
        // the cell coordinate so two candidates at an identical distance cannot be decided by draw
        // order alone (`sort_total!`'s rule, applied to a running max).
        let key = nearest.to_bits();
        let better = match best {
            None => true,
            Some((bk, bc)) => (key, cell.x, cell.y) > (bk, bc.x, bc.y),
        };
        if better {
            best = Some((key, cell));
        }
    }
    best.map(|(_, c)| c)
}

/// Build the run's [`AnomalySites`]. Registered in `RunBuild::Grids`, which is after `World` (the
/// `Dungeon` exists) and before `Populate` (nothing has spawned yet), so every species spawner can read
/// the table without a per-spawner ordering edge.
pub(crate) fn build_anomaly_sites(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    sim: Res<crate::sim::SimTuning>,
) {
    let rules = rules_from_config(&sim);
    let floor = floor_cells(&dungeon);
    let mut rng = crate::rng::seeded(super::PLACEMENT_SEED ^ super::splitmix64(ANOMALY_STREAM));
    let (sites, short) = solve_anomaly_sites(
        &floor,
        dungeon.spawn,
        &rules,
        sim.anomaly_separation,
        &mut rng,
    );

    for (key, placed, wanted) in short {
        // Loud, never silent — the same contract `broadcast::spawn_screens` keeps. A level that cannot
        // hold its authored population under the separation rule is a content fact, and a species that
        // quietly placed fewer is the failure this whole module exists to end.
        warn!(
            "anomaly placement: {key} placed {placed} of {wanted} — no further floor cell \
             {:.1} tiles clear of every other anomaly and past its spawn minimum",
            sim.anomaly_separation
        );
    }
    commands.insert_resource(sites);
}

/// Every floor cell of the dungeon, in a stable total order.
///
/// The raster walk already produces `(x, y)` order; the `sort_total!` states the invariant the
/// selection relies on rather than trusting the loop to keep producing it, and would panic naming this
/// site if the key ever stopped being unique. Grid geometry only — never an ECS query.
pub fn floor_cells(dungeon: &Dungeon) -> Vec<IVec2> {
    let mut floor: Vec<IVec2> = Vec::new();
    for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if dungeon.is_floor(cell) {
                floor.push(cell);
            }
        }
    }
    crate::sort_total!(&mut floor, |c: &IVec2| (c.x, c.y));
    floor
}

/// Compile the per-species rules from the config each species already owns.
///
/// The declared order here is the placement order, and it is deliberate: **scarcest and most
/// constrained first**. SCP-610 carries by far the largest `spawn_min_dist`, so it has the smallest
/// eligible set and would be the one starved if it went last; the crab nests are the most numerous and
/// least fussy, so they fill in around whatever the named anomalies took.
fn rules_from_config(sim: &crate::sim::SimTuning) -> Vec<AnomalyRule> {
    vec![
        AnomalyRule {
            key: crate::scp610::ANOMALY_KEY,
            count: crate::scp610::BLOOM_COUNT,
            min_from_spawn: crate::scp610::SPAWN_MIN_DIST as f32,
        },
        AnomalyRule {
            key: crate::scp1048::ANOMALY_KEY,
            count: sim.scp1048.count,
            min_from_spawn: sim.scp1048.spawn_min_dist,
        },
        AnomalyRule {
            key: crate::scp999::ANOMALY_KEY,
            count: sim.scp999.count,
            min_from_spawn: sim.scp999.spawn_min_dist,
        },
        AnomalyRule {
            key: crate::enemy::ANOMALY_KEY,
            count: crate::enemy::ENEMY_COUNT,
            min_from_spawn: crate::enemy::MIN_SPAWN_DIST,
        },
        AnomalyRule {
            key: crate::crab::ANOMALY_KEY,
            count: crate::crab::CRAB_CLUSTERS,
            min_from_spawn: crate::crab::CRAB_MIN_SPAWN_DIST,
        },
    ]
}

#[cfg(test)]
#[path = "anomalies_tests.rs"]
mod tests;
