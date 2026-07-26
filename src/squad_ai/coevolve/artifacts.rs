//! Human-reviewable artifacts: the elite docs a bake writes out so an evolved config can be read and
//! diffed by a person, not just applied.
//! Split out of the former single-file `coevolve.rs`; a pure move (FVS-N-3).

use super::*;

// ── Human-reviewable artifacts ───────────────────────────────────────────────────────────────────
//
// An elite is committed as RON in the same shape a designer authors. This is the reward-hacking guard
// (Skalse et al.): before an archive ships, a human reads the diff and can refuse it. Opaque weights
// would make that impossible, and the project's one-path rule forbids "magic results that are hard to
// debug".

/// One squad elite, decoded back into authored form.
#[derive(Serialize)]
pub struct SquadEliteDoc {
    pub cell: (usize, usize),
    pub aggression: f32,
    pub exploration: f32,
    pub fitness: f32,
    /// `(role, repertoire)` pairs in `RoleId::ALL` order — the `roles.ron` shape.
    pub roles: Vec<(RoleId, crate::squad_ai::role::RoleDef)>,
}

/// One swarm elite, decoded back into authored form.
#[derive(Serialize)]
pub struct SwarmEliteDoc {
    pub cell: (usize, usize),
    pub aggression: f32,
    pub persistence: f32,
    pub fitness: f32,
    pub crab: Vec<Behavior>,
    pub scout: Vec<Behavior>,
    pub smiley: Vec<Behavior>,
}

/// The archive as it lands on disk.
#[derive(Serialize)]
pub struct ArchiveDoc<E> {
    pub resolution: usize,
    pub coverage: usize,
    pub qd_score: f32,
    pub elites: Vec<E>,
}

/// Decode every squad elite for review/commit.
pub fn squad_archive_doc(t: &Templates, pop: &Population<SquadGenome>) -> Result<ArchiveDoc<SquadEliteDoc>, String> {
    let mut elites = Vec::new();
    for (cell, elite) in pop.archive.iter() {
        let genome = pop.get(elite.genome).ok_or("dangling elite handle")?;
        let mut roles = Vec::new();
        for ((role, template), g) in RoleId::ALL.iter().zip(&t.roles).zip(&genome.0) {
            roles.push((*role, crate::squad_ai::role::RoleDef { behaviors: decode(template, g)? }));
        }
        elites.push(SquadEliteDoc {
            cell: *cell,
            aggression: elite.descriptor.aggression,
            exploration: elite.descriptor.exploration,
            fitness: elite.fitness,
            roles,
        });
    }
    Ok(ArchiveDoc {
        resolution: pop.archive.resolution(),
        coverage: pop.archive.coverage(),
        qd_score: pop.archive.qd_score(),
        elites,
    })
}

/// Decode every swarm elite for review/commit.
pub fn swarm_archive_doc(t: &Templates, pop: &Population<SwarmGenome>) -> Result<ArchiveDoc<SwarmEliteDoc>, String> {
    let mut elites = Vec::new();
    for (cell, elite) in pop.archive.iter() {
        let g = pop.get(elite.genome).ok_or("dangling elite handle")?;
        elites.push(SwarmEliteDoc {
            cell: *cell,
            aggression: elite.descriptor.aggression,
            // The archive's second axis carries `persistence` for the swarm (see `swarm_descriptor`).
            persistence: elite.descriptor.exploration,
            fitness: elite.fitness,
            crab: decode(&t.crab, &g.crab)?,
            scout: decode(&t.scout, &g.scout)?,
            smiley: decode(&t.smiley, &g.smiley)?,
        });
    }
    Ok(ArchiveDoc {
        resolution: pop.archive.resolution(),
        coverage: pop.archive.coverage(),
        qd_score: pop.archive.qd_score(),
        elites,
    })
}

/// One world elite, decoded back into **all four** of its config slices — a readable RON diff of the
/// shipped world's dials (the reward-hacking guard: a human reads what the search found before it ships).
///
/// Every slice `world_genome` encodes must appear here. The rollout scores the whole [`WorldConfig`], so a
/// slice omitted from this doc is a knob the search optimised and the game can never ship — the elite's
/// reported fitness would not be reproducible from the config it bakes. (`mold` + `almond` were exactly
/// that until they were added here: 23 of the genome's then-102 knobs evaluated, then dropped on write.)
#[derive(Serialize)]
pub struct WorldEliteDoc {
    pub cell: (usize, usize),
    /// The archive's axes carry the world's descriptor (`world_descriptor`): total cross-species deaths ×
    /// total cross-species lives, each normalised into `[0,1]`. `BehaviorDescriptor`'s generic
    /// `aggression`/`exploration` fields hold them respectively.
    pub total_deaths: f32,
    pub total_lives: f32,
    pub fitness: f32,
    pub ai: crate::ai::tuning::AiTuning,
    pub sim: crate::sim::SimTuning,
    pub mold: crate::mold::MoldConfig,
    /// The evolvable gameplay subset of `almond_water` (`AlmondWaterDynamics`), not the full config: the
    /// structural + visual knobs are not evolved and stay shipped.
    pub almond: crate::almond_water::AlmondWaterDynamics,
    /// The evolvable gameplay subset of `lighting` (`LightingDynamics`) — likewise not the full config.
    pub lighting: crate::light::LightingDynamics,
}

/// Decode every world elite for review/commit — each is a readable diff of the shipped world's dials.
pub fn world_archive_doc(pop: &Population<WorldGenome>) -> Result<ArchiveDoc<WorldEliteDoc>, String> {
    let mut elites = Vec::new();
    for (cell, elite) in pop.archive.iter() {
        let g = pop.get(elite.genome).ok_or("dangling elite handle")?;
        let wc = world_genome::decode(g)?;
        elites.push(WorldEliteDoc {
            cell: *cell,
            total_deaths: elite.descriptor.aggression,
            total_lives: elite.descriptor.exploration,
            fitness: elite.fitness,
            ai: wc.ai,
            sim: wc.sim,
            mold: wc.mold,
            almond: wc.almond,
            lighting: wc.lighting,
        });
    }
    Ok(ArchiveDoc {
        resolution: pop.archive.resolution(),
        coverage: pop.archive.coverage(),
        qd_score: pop.archive.qd_score(),
        elites,
    })
}

/// Sweep the **authored** brains to build the player's baseline expectation.
///
/// This is `P(mode | context)` for the game as shipped: the model every prior encounter has trained the
/// player on. Surprise is measured against it and it never moves during a search — a reference that
/// drifted with the population would make "surprising" mean only "different from last generation".
pub fn sweep_prior(t: &Templates, seeds: &[u64], episode_ticks: u32) -> Result<ModePrior, String> {
    let squad = SquadGenome::authored(t);
    let swarm = SwarmGenome::authored(t);
    let mut prior = ModePrior::default();
    for &seed in seeds {
        let r = rollout(brains_of(t, &squad, &swarm)?, None, None, None, seed, episode_ticks);
        prior.observe(&r.trace);
    }
    Ok(prior)
}
