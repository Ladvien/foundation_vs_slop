//! The **level search**: a single-population MAP-Elites (Quality-Diversity) loop that evolves the level
//! genome under the static level-quality objective.
//!
//! It is *standalone*, not co-evolutionary: the fitness ([`level_eval::evaluate`]) is a fixed function of
//! the generated level, independent of the squad/swarm brains, so there is no opponent sampling and no
//! non-stationarity — a plain propose → evaluate → archive loop (Mouret & Clune 2015, MAP-Elites). It
//! reuses [`super::coevolve::Population`] (archive + genome store) and the archive-doc wrapper, so an
//! elite writes to `assets/config/elites_levels.ron` in the same readable shape the other searches use.
//!
//! Feasible-by-construction mutation means most children generate; the loud reject paths
//! (`rejected_infeasible` / `rejected_by_criterion`) count what the bounds and the minimal criterion turn
//! away, so a run that fills nothing is visible rather than silent.

use serde::Serialize;

use crate::config::PlacementDensity;
use crate::dungeon::DungeonConfig;
use crate::mycelia::MyceliaConfig;
use crate::placement::manifest::FurnitureManifest;
use crate::placement::solvers::metropolis::MetropolisWeights;
use crate::rng::seeded;

use super::coevolve::{ArchiveDoc, Population};
use super::level_eval;
use super::level_genome::{self, authored, mutate, LevelBase, LevelGenome};
use super::map_elites::{map_elites_loop, MapElitesResult};
use super::qd::BehaviorDescriptor;

/// Knobs for a level search. Held-in `dungeon_seeds` are the maps every genome is scored across (and must
/// clear the criterion on all of them). `resolution` is the MAP-Elites grid side (`res × res` niches over
/// openness × infestation).
#[derive(Clone, Debug)]
pub struct LevelSearchConfig {
    pub seed: u64,
    pub generations: u32,
    pub batch: u32,
    pub sigma: f32,
    pub resolution: usize,
    pub dungeon_seeds: Vec<u64>,
}

impl Default for LevelSearchConfig {
    fn default() -> Self {
        LevelSearchConfig {
            seed: 0x1E4E1_5EED,
            generations: 40,
            batch: 32,
            sigma: 0.3,
            resolution: 8,
            dungeon_seeds: vec![0x5C09191, 0xA11CE, 0xBEEF],
        }
    }
}

/// The outcome of a level search: the illuminated archive plus reject tallies. Aliases the shared
/// [`MapElitesResult`] at the level genome, so the level and audio searches share one result shape.
pub type LevelSearchResult = MapElitesResult<LevelGenome>;

/// Run the level search. `report(generation, &result)` is called after each generation; `search` itself
/// writes nothing to disk (the driver in `train.rs` does). One path: an infeasible or criterion-failing
/// child is counted and dropped, never scored with a degraded fallback.
pub fn search(
    base: &LevelBase,
    manifest: &FurnitureManifest,
    cfg: &LevelSearchConfig,
    mut report: impl FnMut(u32, &LevelSearchResult),
) -> Result<LevelSearchResult, String> {
    let mut rng = seeded(cfg.seed);
    let authored_g = authored(base);
    // The shipped level must itself be feasible — a loud failure here means the base config is broken.
    level_genome::is_feasible(&authored_g, base)
        .map_err(|e| format!("the shipped level is infeasible: {e}"))?;

    let mut result = LevelSearchResult {
        pop: Population::new(cfg.resolution),
        evaluations: 0,
        rejected_infeasible: 0,
        rejected_by_criterion: 0,
    };

    // The propose → evaluate → archive loop is shared with the audio search; only mutate / feasibility /
    // evaluate vary, supplied here as closures. Level mutation is infallible, so it is wrapped in `Ok`;
    // the (also cheap) generate-and-measure evaluation is the criterion gate.
    map_elites_loop(
        &mut rng,
        &mut result,
        &authored_g,
        cfg.generations,
        cfg.batch,
        "the shipped level failed the minimal criterion on the held-in seeds",
        |parent, rng| Ok(mutate(parent, cfg.sigma, rng)),
        |child| level_genome::is_feasible(child, base).is_ok(),
        |g| {
            level_eval::evaluate(g, base, manifest, &cfg.dungeon_seeds)
                .map(|ev| (BehaviorDescriptor::new(ev.axes.0, ev.axes.1), ev.fitness))
        },
        &mut report,
    )?;
    Ok(result)
}

/// One archived level, decoded to the exact `dungeon:` / `placement.metropolis:` / `placement.density:` /
/// `mycelia:` RON a designer authors by hand — so an elite is a readable diff of level dials, the
/// reward-hacking guard (Skalse et al., arXiv:2209.13085). Copy the slices you want into `config.ron`.
#[derive(Serialize)]
pub struct LevelEliteDoc {
    pub cell: (usize, usize),
    /// Descriptor axis 1 — furniture clutter (normalised pieces/room).
    pub clutter: f32,
    /// Descriptor axis 2 — mould infestation (normalised coverage of floor).
    pub infestation: f32,
    pub fitness: f32,
    pub dungeon: DungeonConfig,
    pub metropolis: MetropolisWeights,
    pub density: PlacementDensity,
    pub mycelia: MyceliaConfig,
}

/// Build the serializable archive document — every elite decoded back to readable config slices.
pub fn level_archive_doc(
    pop: &Population<LevelGenome>,
    base: &LevelBase,
) -> Result<ArchiveDoc<LevelEliteDoc>, String> {
    let mut elites = Vec::new();
    for (cell, elite) in pop.archive.iter() {
        let g = pop.get(elite.genome).ok_or("dangling elite handle")?;
        let p = level_genome::decode(g, base)?;
        elites.push(LevelEliteDoc {
            cell: *cell,
            clutter: elite.descriptor.aggression,
            infestation: elite.descriptor.exploration,
            fitness: elite.fitness,
            dungeon: p.dungeon,
            metropolis: p.metropolis,
            density: p.density,
            mycelia: p.mycelia,
        });
    }
    Ok(ArchiveDoc {
        resolution: pop.archive.resolution(),
        coverage: pop.archive.coverage(),
        qd_score: pop.archive.qd_score(),
        elites,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_search_fills_the_archive_and_writes_readable_elites() {
        // End-to-end: a tiny search over the shipped base illuminates at least a couple of niches and its
        // archive doc serialises to RON. GPU-free; proves the whole loop composes.
        let (base, manifest) = level_eval::load_base().expect("shipped config");
        let cfg = LevelSearchConfig {
            generations: 3,
            batch: 8,
            dungeon_seeds: vec![0x5C09191, 0xA11CE],
            ..Default::default()
        };
        let result = search(&base, &manifest, &cfg, |_, _| {}).expect("search runs");
        assert!(result.pop.archive.coverage() >= 1, "archive should hold ≥1 elite");
        assert!(result.evaluations >= 1);
        let doc = level_archive_doc(&result.pop, &base).expect("archive doc");
        let ron = ron::ser::to_string_pretty(&doc, ron::ser::PrettyConfig::default())
            .expect("elites serialise to RON");
        assert!(ron.contains("dungeon"), "elite RON carries the dungeon slice");
    }
}
