//! **Evolved-elite overlay** — run the game with tuning from an offline-search archive *without* editing
//! `config.ron`. Each evolved dimension has an env var naming an archive (and optionally a cell); when set,
//! the elite is decoded and overlaid onto the loaded [`GameConfig`] (or, for the RL policy, installed as the
//! squad `ActivePolicy`). Unset → the shipped default for that dimension. `config.ron` stays pristine, git
//! clean, and the `*_default_equals_shipped_config` guards unaffected — the ergonomic, reversible way to try
//! any elite the search produced.
//!
//! ```text
//! FVS_BEHAVIOR_ELITE=path.ron          # best-fitness elite
//! FVS_WORLD_ELITE=path.ron#0,7         # a specific archive cell (row,col)
//! FVS_AUDIO_ELITE / FVS_LEVELS_ELITE / FVS_POLICY_ELITE
//! ```
//!
//! The offline archives (`ArchiveDoc<*EliteDoc>`) are `Serialize`-only **and** `#[cfg(feature =
//! "test-harness")]`-gated, so the shipped game binary cannot name those types. This module therefore
//! defines its own minimal **`Deserialize` mirrors**; serde ignores the archive fields it does not name
//! (`resolution` / `coverage` / `qd_score`, and the descriptor axes on each elite). Every slice *type* it
//! overlays is ungated. One path: a missing file, parse failure, empty archive, or absent cell is a loud
//! `Err`, never a silent fall-back to the shipped value.

use serde::Deserialize;

use crate::ai::tuning::AiTuning;
use crate::almond_water::AlmondWaterDynamics;
use crate::audio_tuning::AudioTuning;
use crate::behavior_tuning::BehaviorTuning;
use crate::config::{GameConfig, PlacementDensity};
use crate::dungeon::DungeonConfig;
use crate::light::LightingDynamics;
use crate::mold::MoldConfig;
use crate::mycelia::MyceliaConfig;
use crate::placement::solvers::metropolis::MetropolisWeights;
use crate::sim::SimTuning;
use crate::squad_ai::policy::NeuralPolicy;

/// Env var naming a `behavior` archive to overlay onto `gc.behavior`.
pub const BEHAVIOR_ENV: &str = "FVS_BEHAVIOR_ELITE";
/// Env var naming a `world` archive to overlay onto `gc.ai_tuning` + `gc.sim` + `gc.mold` +
/// `gc.almond_water` (the evolvable subset) — all four slices `world_genome` encodes.
pub const WORLD_ENV: &str = "FVS_WORLD_ELITE";
/// Env var naming an `audio` archive to overlay onto `gc.audio`.
pub const AUDIO_ENV: &str = "FVS_AUDIO_ELITE";
/// Env var naming a `levels` archive to overlay onto `gc.dungeon` + `gc.mycelia` + `gc.placement.*`.
pub const LEVELS_ENV: &str = "FVS_LEVELS_ELITE";
/// Env var naming a `policy` (neuroevolution) archive to install as the squad `ActivePolicy`.
pub const POLICY_ENV: &str = "FVS_POLICY_ELITE";

// ── minimal Deserialize mirrors of the archive docs (serde ignores the unnamed fields) ──

#[derive(Deserialize)]
struct Archive<E> {
    elites: Vec<E>,
}

#[derive(Deserialize)]
struct BehaviorEntry {
    cell: (usize, usize),
    fitness: f32,
    behavior: BehaviorTuning,
}
/// All four slices `world_genome` encodes. Every field is **required**: a `serde(default)` here would let a
/// pre-`mold`/`almond` archive load and silently ship only part of what the search evolved — the exact bug
/// this mirror existed with for 23 knobs. A stale archive must fail loudly and be regenerated.
#[derive(Deserialize)]
struct WorldEntry {
    cell: (usize, usize),
    fitness: f32,
    ai: AiTuning,
    sim: SimTuning,
    mold: MoldConfig,
    almond: AlmondWaterDynamics,
    lighting: LightingDynamics,
}
#[derive(Deserialize)]
struct AudioEntry {
    cell: (usize, usize),
    fitness: f32,
    audio: AudioTuning,
}
#[derive(Deserialize)]
struct LevelEntry {
    cell: (usize, usize),
    fitness: f32,
    dungeon: DungeonConfig,
    metropolis: MetropolisWeights,
    density: PlacementDensity,
    mycelia: MyceliaConfig,
}
#[derive(Deserialize)]
struct PolicyEntry {
    cell: (usize, usize),
    fitness: f32,
    weights: Vec<f32>,
}

/// Cell + fitness accessors, so elite selection is one generic function over every entry type.
trait Elite {
    fn cell(&self) -> (usize, usize);
    fn fitness(&self) -> f32;
}
macro_rules! elite {
    ($t:ty) => {
        impl Elite for $t {
            fn cell(&self) -> (usize, usize) {
                self.cell
            }
            fn fitness(&self) -> f32 {
                self.fitness
            }
        }
    };
}
elite!(BehaviorEntry);
elite!(WorldEntry);
elite!(AudioEntry);
elite!(LevelEntry);
elite!(PolicyEntry);

/// Split an env spec `<path>` or `<path>#row,col` into its path and optional cell.
fn parse_spec(spec: &str) -> Result<(String, Option<(usize, usize)>), String> {
    match spec.split_once('#') {
        Some((path, cell)) => Ok((path.trim().to_string(), Some(parse_cell(cell)?))),
        None => Ok((spec.trim().to_string(), None)),
    }
}

/// Parse a `row,col` cell selector.
fn parse_cell(s: &str) -> Result<(usize, usize), String> {
    let (r, c) = s.split_once(',').ok_or_else(|| format!("cell {s:?} must be `row,col`"))?;
    let r = r.trim().parse::<usize>().map_err(|e| format!("cell row {r:?}: {e}"))?;
    let c = c.trim().parse::<usize>().map_err(|e| format!("cell col {c:?}: {e}"))?;
    Ok((r, c))
}

/// Index of the requested elite: the named cell, or the highest-fitness one. Loud on empty/absent.
fn pick_index<E: Elite>(elites: &[E], cell: Option<(usize, usize)>, path: &str) -> Result<usize, String> {
    if elites.is_empty() {
        return Err(format!("{path}: archive has no elites"));
    }
    match cell {
        Some(c) => elites
            .iter()
            .position(|e| e.cell() == c)
            .ok_or_else(|| format!("{path}: no elite at cell {c:?}")),
        None => {
            let mut best = 0usize;
            let mut best_fit = f32::NEG_INFINITY;
            for (i, e) in elites.iter().enumerate() {
                if e.fitness() > best_fit {
                    best_fit = e.fitness();
                    best = i;
                }
            }
            Ok(best)
        }
    }
}

/// Read an env spec, parse the archive as `Archive<E>`, and return the selected elite **by value** (moved
/// out, no clone). `dim` names the dimension in error messages.
fn read_elite<E: Elite + serde::de::DeserializeOwned>(spec: &str, dim: &str) -> Result<E, String> {
    let (path, cell) = parse_spec(spec)?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{dim} elite: cannot read {path}: {e}"))?;
    let mut arch: Archive<E> =
        ron::from_str(&text).map_err(|e| format!("{path}: not a {dim} archive: {e}"))?;
    let idx = pick_index(&arch.elites, cell, &path)?;
    Ok(arch.elites.swap_remove(idx))
}

/// A config-slice evolved dimension (the ones `train apply` can permanently bake and `FVS_*_ELITE` can
/// overlay). Policy is deliberately absent — a `NeuralPolicy` is not a config slice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    Behavior,
    World,
    Audio,
    Levels,
}

impl Dim {
    /// Parse a CLI dimension name.
    pub fn parse(s: &str) -> Result<Dim, String> {
        match s {
            "behavior" | "behaviour" => Ok(Dim::Behavior),
            "world" => Ok(Dim::World),
            "audio" => Ok(Dim::Audio),
            "levels" | "level" => Ok(Dim::Levels),
            other => Err(format!("unknown dim {other:?} (expected behavior|world|audio|levels)")),
        }
    }
}

/// Overlay one dimension's elite (`spec` = `<path>[#row,col]`) onto `gc`, in place. Returns a human-readable
/// description of what was applied. Shared by the `FVS_*_ELITE` overlay and the `train apply` bake, so both
/// decode + map elites identically (and inherit the same per-slice validation downstream).
pub fn apply_dim(gc: &mut GameConfig, dim: Dim, spec: &str) -> Result<String, String> {
    Ok(match dim {
        Dim::Behavior => {
            let e: BehaviorEntry = read_elite(spec, "behaviour")?;
            let (cell, fit) = (e.cell, e.fitness);
            gc.behavior = e.behavior;
            format!("behaviour <- {spec} (cell {cell:?}, fitness {fit:.3})")
        }
        Dim::World => {
            let e: WorldEntry = read_elite(spec, "world")?;
            let (cell, fit) = (e.cell, e.fitness);
            // Mirrors `sim_harness`'s install of a `WorldConfig` — the search scored the elite through that
            // exact mapping, so anything omitted here makes the baked config disagree with the fitness.
            gc.ai_tuning = e.ai; // NB: WorldConfig.ai maps to GameConfig.ai_tuning
            gc.sim = e.sim;
            gc.mold = e.mold;
            e.almond.apply_to(&mut gc.almond_water); // evolvable gameplay knobs only
            e.lighting.apply_to(&mut gc.lighting); // ditto — visual light knobs stay authored
            format!(
                "world (ai_tuning+sim+mold+almond_water+lighting) <- {spec} (cell {cell:?}, fitness {fit:.3})"
            )
        }
        Dim::Audio => {
            let e: AudioEntry = read_elite(spec, "audio")?;
            let (cell, fit) = (e.cell, e.fitness);
            gc.audio = e.audio;
            format!("audio <- {spec} (cell {cell:?}, fitness {fit:.3})")
        }
        Dim::Levels => {
            let e: LevelEntry = read_elite(spec, "levels")?;
            let (cell, fit) = (e.cell, e.fitness);
            gc.dungeon = e.dungeon;
            gc.mycelia = e.mycelia;
            gc.placement.metropolis = e.metropolis;
            gc.placement.density = e.density;
            format!("levels (dungeon+placement+mycelia) <- {spec} (cell {cell:?}, fitness {fit:.3})")
        }
    })
}

/// Overlay any set config-dimension elites (behaviour / world / audio / levels) onto `gc`, in place.
/// Returns a human-readable line per applied dimension (for logging). Called by `config::load_game_config`
/// **before** the per-slice validators, so an overlaid slice is validated on the same one path.
pub fn overlay_config_elites(gc: &mut GameConfig) -> Result<Vec<String>, String> {
    let mut applied = Vec::new();
    for (envname, dim) in
        [(BEHAVIOR_ENV, Dim::Behavior), (WORLD_ENV, Dim::World), (AUDIO_ENV, Dim::Audio), (LEVELS_ENV, Dim::Levels)]
    {
        if let Some(spec) = env(envname) {
            applied.push(apply_dim(gc, dim, &spec)?);
        }
    }
    Ok(applied)
}

/// If [`POLICY_ENV`] is set, decode the elite's weights into a [`NeuralPolicy`] for the squad `ActivePolicy`
/// seam (installed by `lib::run` before `SquadAiPlugin`). `Ok(None)` when unset.
pub fn load_policy_elite() -> Result<Option<(NeuralPolicy, String)>, String> {
    let Some(spec) = env(POLICY_ENV) else {
        return Ok(None);
    };
    let e: PolicyEntry = read_elite(&spec, "policy")?;
    let np = NeuralPolicy::from_weights(&e.weights).map_err(|err| format!("{spec}: {err}"))?;
    Ok(Some((np, format!("policy <- {spec} (cell {:?}, fitness {:.3})", e.cell, e.fitness))))
}

/// Read a non-empty, trimmed env var, or `None`.
fn env(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every slice the world genome encodes must survive `apply_dim`.**
    ///
    /// This module shipped with `WorldEntry` naming only `ai` + `sim`, so `mold` and `almond` were silently
    /// dropped: 23 of the world genome's 102 knobs were scored by the search and could never reach the game,
    /// and an elite's archived fitness was unreproducible from the config `train apply` baked. Nothing failed
    /// loudly, because serde deliberately ignores archive fields a mirror does not name (see the module doc)
    /// — so the omission is invisible unless a test asserts each slice individually. `apply_dim` had no test
    /// at all, which is why it went unnoticed.
    ///
    /// Assert every slice, one at a time. A blanket "it parsed" check would not have caught the original bug.
    #[test]
    fn world_apply_dim_lands_every_evolvable_slice() {
        let mut gc = match crate::config::load_game_config() {
            Ok(gc) => gc,
            // The overlay env vars would double-apply; `apply_archive` refuses for the same reason.
            Err(e) => panic!("load the shipped config: {e}"),
        };

        // Move every evolvable slice OFF its shipped value, so a dropped slice can't pass by coincidence.
        let mut ai = gc.ai_tuning;
        ai.rally.decay = 1.25;
        let mut sim = gc.sim;
        sim.fear.per_crab = 0.375;
        let mut mold = gc.mold;
        mold.dim_light = 0.125;
        mold.seep_boost = 4.25;
        let mut almond = AlmondWaterDynamics::from_config(&gc.almond_water);
        almond.strong_seep = 12.5;
        almond.heal_rate = 7.25;
        let lighting = LightingDynamics { field_intensity: 2.5, photophobic_gain: 11.0 };

        let doc = format!(
            "(resolution: 8, coverage: 1, qd_score: 1.0, elites: [(cell: (0, 0), total_deaths: 0.0, \
             total_lives: 0.0, fitness: 0.5, ai: {}, sim: {}, mold: {}, almond: {}, lighting: {})])",
            ron::to_string(&ai).expect("ser ai"),
            ron::to_string(&sim).expect("ser sim"),
            ron::to_string(&mold).expect("ser mold"),
            ron::to_string(&almond).expect("ser almond"),
            ron::to_string(&lighting).expect("ser lighting"),
        );
        let path = std::env::temp_dir().join("fvs_apply_dim_world_roundtrip.ron");
        std::fs::write(&path, &doc).expect("write the test archive");
        let spec = path.to_str().expect("utf-8 temp path").to_string();

        let desc = apply_dim(&mut gc, Dim::World, &spec).expect("apply the world elite");
        let _ = std::fs::remove_file(&path);

        assert_eq!(gc.ai_tuning, ai, "ai_tuning did not land");
        assert_eq!(gc.sim, sim, "sim did not land");
        assert_eq!(gc.mold, mold, "mold did not land — the original 7-knob drop");
        assert_eq!(
            AlmondWaterDynamics::from_config(&gc.almond_water),
            almond,
            "almond dynamics did not land — the original 16-knob drop"
        );
        assert_eq!(LightingDynamics::from_config(&gc.lighting), lighting, "lighting did not land");
        assert!(desc.contains("cell (0, 0)"), "the description should name the cell: {desc}");
    }

    /// A stale archive (one written before a slice was added) must fail LOUDLY, not load and ship a partial
    /// world. This is why `WorldEntry`'s fields carry no `#[serde(default)]`: a default would silently
    /// resurrect the dropped-knob bug on every pre-existing archive.
    #[test]
    fn world_archive_missing_a_slice_is_a_loud_error() {
        let mut gc = crate::config::load_game_config().expect("load the shipped config");
        let doc = format!(
            "(resolution: 8, coverage: 1, qd_score: 1.0, elites: [(cell: (0, 0), fitness: 0.5, ai: {}, \
             sim: {})])",
            ron::to_string(&gc.ai_tuning).expect("ser ai"),
            ron::to_string(&gc.sim).expect("ser sim"),
        );
        let path = std::env::temp_dir().join("fvs_apply_dim_world_stale.ron");
        std::fs::write(&path, &doc).expect("write the stale archive");
        let spec = path.to_str().expect("utf-8 temp path").to_string();

        let err = apply_dim(&mut gc, Dim::World, &spec)
            .expect_err("an archive without mold/almond/lighting must be rejected");
        let _ = std::fs::remove_file(&path);
        assert!(err.contains("world"), "the error should name the dimension: {err}");
    }

    #[test]
    fn parse_cell_and_spec() {
        assert_eq!(parse_cell("0,7").unwrap(), (0, 7));
        assert_eq!(parse_cell(" 3 , 4 ").unwrap(), (3, 4));
        assert!(parse_cell("nope").is_err());
        assert_eq!(parse_spec("a/b.ron").unwrap(), ("a/b.ron".to_string(), None));
        assert_eq!(parse_spec("a/b.ron#1,2").unwrap(), ("a/b.ron".to_string(), Some((1, 2))));
    }

    // A tiny Deserialize+Elite entry, to test selection and the ignore-unknown-fields assumption without a
    // full BehaviorTuning.
    #[derive(Deserialize)]
    struct TestEntry {
        cell: (usize, usize),
        fitness: f32,
    }
    elite!(TestEntry);

    #[test]
    fn archive_ignores_unknown_fields() {
        // The load-bearing assumption: the mirror reads only the fields it names; the real archive's
        // resolution/coverage/qd_score and per-elite descriptor axes are silently skipped.
        let ron = "(resolution: 8, coverage: 2, qd_score: 0.5, elites: [\
                   (cell: (0, 7), aggression: 0.1, persistence: 0.9, fitness: 0.42)])";
        let arch: Archive<TestEntry> = ron::from_str(ron).expect("mirror must ignore unknown fields");
        assert_eq!(arch.elites.len(), 1);
        assert_eq!(arch.elites[0].cell, (0, 7));
        assert!((arch.elites[0].fitness - 0.42).abs() < 1e-6);
    }

    #[test]
    fn pick_by_cell_and_by_best_fitness() {
        let elites = vec![
            TestEntry { cell: (0, 0), fitness: 0.1 },
            TestEntry { cell: (0, 7), fitness: 0.9 },
            TestEntry { cell: (1, 3), fitness: 0.5 },
        ];
        assert_eq!(pick_index(&elites, None, "x").unwrap(), 1, "best fitness");
        assert_eq!(pick_index(&elites, Some((1, 3)), "x").unwrap(), 2, "named cell");
        assert!(pick_index(&elites, Some((9, 9)), "x").is_err(), "absent cell");
        assert!(pick_index::<TestEntry>(&[], None, "x").is_err(), "empty archive");
    }
}
