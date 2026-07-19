//! The **audio search**: a single-population MAP-Elites (Quality-Diversity) loop that evolves the
//! [`AudioGenome`] under the witnessed-learnable-surprise objective (`super::audio_eval`).
//!
//! Structurally it is the level search's plain propose → evaluate → archive loop (Mouret & Clune 2015,
//! MAP-Elites), reusing [`super::coevolve::Population`] + [`ArchiveDoc`] — but its fitness is a *behavioural*
//! full-simulation rollout, not a static metric, so it takes the frozen baseline `prior` (like the world
//! population) rather than being self-contained. It is NOT co-evolutionary: the brains and world are the
//! authored baseline, and only the `audio:` slice moves, so there is no opponent sampling and no
//! non-stationarity — one config knob set, illuminated across the swarm-behaviour descriptor.
//!
//! Feasible-by-construction mutation means most children generate; the loud reject paths
//! (`rejected_infeasible` / `rejected_by_criterion`) count what the bounds and the minimal criterion turn
//! away, so a run that fills nothing is visible rather than silent.

use serde::Serialize;

use crate::audio_tuning::AudioTuning;
use crate::rng::seeded;

use super::audio_eval;
use super::audio_genome::{self, authored, mutate, AudioGenome};
use super::coevolve::{ArchiveDoc, Population};
use super::map_elites::{map_elites_loop, MapElitesResult};
use super::qd::BehaviorDescriptor;
use super::surprise::ModePrior;

/// Knobs for an audio search. `dungeon_seeds` are the worlds each genome is scored across (the learnability
/// pair uses the first two, which must differ); `resolution` is the MAP-Elites grid side over the
/// swarm-behaviour axes (aggression × persistence).
#[derive(Clone, Debug)]
pub struct AudioSearchConfig {
    pub seed: u64,
    pub generations: u32,
    pub batch: u32,
    pub sigma: f32,
    pub resolution: usize,
    pub dungeon_seeds: Vec<u64>,
    pub episode_ticks: u32,
    /// Convergence early-stop patience (generations without QD-score gain); `0` disables. See
    /// [`crate::squad_ai::qd::PlateauStop`].
    pub patience: u32,
}

impl Default for AudioSearchConfig {
    fn default() -> Self {
        AudioSearchConfig {
            seed: 0xA0D10_5EED,
            generations: 40,
            batch: 32,
            sigma: 0.3,
            resolution: 8,
            dungeon_seeds: crate::squad_ai::coevolve::HELD_IN_SEEDS.to_vec(),
            // The measured minimal-criterion floor (`tests/search_calibration.rs`, and `SearchConfig`'s own
            // default): below it the authored squad takes no damage on some held-in worlds, so the criterion
            // rejects every candidate and the archive stays empty (Mouret & Clune 2015 — the evaluation must
            // let the behaviour play out; Yannakakis et al. 2019 on the minimal criterion in constrained QD).
            episode_ticks: 7200,
            patience: 0,
        }
    }
}

/// The outcome of an audio search: the illuminated archive plus reject tallies. Aliases the shared
/// [`MapElitesResult`] at the audio genome, so the audio and level searches share one result shape.
pub type AudioSearchResult = MapElitesResult<AudioGenome>;

/// Run the audio search. `report(generation, &result)` is called after each generation; `search` itself
/// writes nothing to disk (the `train.rs` driver does). One path: an infeasible or criterion-failing child
/// is counted and dropped, never scored with a degraded fallback.
pub fn search(
    prior: &ModePrior,
    cfg: &AudioSearchConfig,
    mut report: impl FnMut(u32, &AudioSearchResult),
) -> Result<AudioSearchResult, String> {
    if cfg.dungeon_seeds.len() < 2 {
        return Err(
            "audio search needs >= 2 dungeon seeds: the learnability pair must run on DIFFERENT worlds, \
             or fitness measures a memorised map rather than a behaviour"
                .into(),
        );
    }
    let mut rng = seeded(cfg.seed);
    let authored_g = authored();
    // The shipped audio config must itself be feasible — a loud failure here means the base is broken.
    audio_genome::is_feasible(&authored_g)
        .map_err(|e| format!("the shipped audio config is infeasible: {e}"))?;

    let mut result = AudioSearchResult {
        pop: Population::new(cfg.resolution),
        evaluations: 0,
        rejected_infeasible: 0,
        rejected_by_criterion: 0,
    };

    // The propose → evaluate → archive loop is shared with the level search; only mutate / feasibility /
    // evaluate vary, supplied here as closures. Audio mutation is fallible, so its `Result` flows straight
    // through; the criterion gate is the (expensive) full-sim behavioural rollout.
    map_elites_loop(
        &mut rng,
        &mut result,
        &authored_g,
        cfg.generations,
        cfg.batch,
        cfg.patience,
        "the shipped audio config failed the minimal criterion on the held-in seeds",
        |parent, rng| mutate(parent, cfg.sigma, rng),
        |child| audio_genome::is_feasible(child).is_ok(),
        |g| {
            audio_eval::evaluate(g, prior, &cfg.dungeon_seeds, cfg.episode_ticks)
                .map(|ev| (BehaviorDescriptor::new(ev.axes.0, ev.axes.1), ev.fitness))
        },
        &mut report,
    )?;
    Ok(result)
}

/// One archived audio config, decoded to the exact `audio:` RON a designer authors by hand — so an elite
/// is a readable diff of audio dials, the reward-hacking guard (Skalse et al., arXiv:2209.13085). Copy the
/// slice into `config.ron` to ship it.
#[derive(Serialize)]
pub struct AudioEliteDoc {
    pub cell: (usize, usize),
    /// Descriptor axis 1 — swarm aggression.
    pub aggression: f32,
    /// Descriptor axis 2 — swarm persistence.
    pub persistence: f32,
    pub fitness: f32,
    pub audio: AudioTuning,
}

/// Build the serializable archive document — every elite decoded back to a readable `AudioTuning` slice.
pub fn audio_archive_doc(pop: &Population<AudioGenome>) -> Result<ArchiveDoc<AudioEliteDoc>, String> {
    let mut elites = Vec::new();
    for (cell, elite) in pop.archive.iter() {
        let g = pop.get(elite.genome).ok_or("dangling elite handle")?;
        let audio = audio_genome::decode(g)?;
        elites.push(AudioEliteDoc {
            cell: *cell,
            aggression: elite.descriptor.aggression,
            persistence: elite.descriptor.exploration,
            fitness: elite.fitness,
            audio,
        });
    }
    Ok(ArchiveDoc {
        resolution: pop.archive.resolution(),
        coverage: pop.archive.coverage(),
        qd_score: pop.archive.qd_score(),
        elites,
    })
}
