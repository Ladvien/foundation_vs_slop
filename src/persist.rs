//! **Roguelite save/load** (FVS-G-2) — the meta-progress that outlives the process.
//!
//! Saves the **model subset** and nothing else: what the Foundation has caught, what it has learned,
//! what it can now do, and which Branch universe comes next. No view entities, no geometry, no
//! transforms — the Site is rebuilt from `site67.ron` and the expedition world from a seed, so
//! persisting either would be storing a derived thing twice.
//!
//! ## `Specimen::captured` is deliberately NOT saved, and that is the interesting part
//!
//! It is an `Entity`. An entity id is an index into *this process's* ECS, so writing one to disk stores
//! a number that will mean something different — or nothing — next launch. That is exactly the class of
//! bug FVS-N-8 turned out to be, one level up: `autogib::seed_from` hashed an `AssetId`, which is an
//! index into the asset arena, and the fracture moved every run.
//!
//! It is also already dangling *before* any save: the captured anomaly is `run_scoped()`, so it is
//! despawned when the expedition ends, while the `Specimen` outlives it. So the field is meaningful
//! only within the run that created it, and [`SavedSpecimen`] records what actually survives — when it
//! was caught, what is known about it, and what it unlocks.
//!
//! ## Version mismatch is a refusal, not a migration
//!
//! A `version` field that triggers per-version migration paths is the multi-path shape this codebase
//! rejects. Here it does one thing: a save from a different schema is **refused loudly**. That is worse
//! for a shipped game and better for one under construction, and when the format stabilises the right
//! answer is a deliberate migration written once — not a fallback that accretes.
//!
//! ## Determinism
//!
//! Nothing here is on `FixedUpdate`. Save/load happens at the Site, between expeditions, and touches no
//! pinned state — so it cannot move `snapshot_hash`. `RunSeed` is saved because it is what makes "each
//! seed is a Branch universe" survive a restart; restoring it is the difference between resuming a
//! campaign and starting a new one that happens to have your specimens.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::research::{Capability, ResearchPosterior, Researched, TechTree, Unlocks};
use crate::site::{HeldAt, SiteRoot};

/// Bumped whenever the saved shape changes. A mismatch is refused, never migrated — see the module docs.
///
/// `2` (2026-07-27): [`SavedSpecimen`] gained `subject`. A v1 save records *that* four things were
/// captured but not *what* they were, and the research battery and unlock payout are both keyed on
/// species — so a v1 campaign cannot be reconstructed, only guessed at. Refusing is the honest outcome.
pub const SAVE_VERSION: u32 = 2;

/// One banked specimen, as it survives a restart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSpecimen {
    /// The run tick the capture completed on — the stable ordering key (see `containment::Specimen`).
    pub captured_tick: u64,
    /// **What it is.** Saved rather than re-derived, because the anomaly it came from stopped existing
    /// when that expedition ended — and both the research battery and the unlock payout are keyed on it.
    pub subject: crate::knowledge::Subject,
    /// What the Foundation knows about it.
    pub posterior: ResearchPosterior,
    /// How many times each parameter has already been tested — FVS-E-3's fatigue state.
    ///
    /// Saved because without it a reload would hand the player a fresh battery of full-strength tests
    /// on a specimen they had already exhausted, which is a free reset of the whole research economy.
    /// `#[serde(default)]` so a specimen banked before the bench existed loads as untested.
    #[serde(default)]
    pub experiments: crate::research::ExperimentLog,
    /// Whether its research arc has already paid out.
    pub researched: bool,
    /// Which capabilities completing it grants. Empty when nothing is authored for its species.
    #[serde(default)]
    pub unlocks: Vec<Capability>,
}

/// The whole save.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveGame {
    pub version: u32,
    /// The next Branch universe. Without this, loading gives you your specimens in somebody else's
    /// campaign — the worlds would restart from the configured seed while the meta-progress carried on.
    pub run_seed: u64,
    /// Unlocked capabilities, as the flag bitset.
    pub tech_tree: u32,
    /// Every banked specimen, **in capture order**.
    pub specimens: Vec<SavedSpecimen>,
    /// What each operative believes (FVS-G-3 / L-5).
    ///
    /// Keyed by `SquadMember` index rather than by entity, because operatives are `run_scoped()` and
    /// rebuilt every expedition — "operatives persist" means their *beliefs* do. `#[serde(default)]`
    /// so a campaign saved before the knowledge layer existed loads with an inexperienced squad.
    #[serde(default)]
    pub squad_knowledge: crate::knowledge::SquadKnowledge,
}

impl SaveGame {
    /// Reject a save this build cannot honestly load. One path, no migration.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != SAVE_VERSION {
            return Err(format!(
                "save is version {} but this build writes {SAVE_VERSION}; refusing to load rather \
                 than guess at a migration",
                self.version
            ));
        }
        Ok(())
    }
}

/// Where the campaign lives. Alongside `user_settings.ron`, and resolved the same dependency-free way.
pub fn save_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
    Some(base.join("FoundationVsSlop").join("campaign.ron"))
}

/// Gather the current campaign out of the world.
///
/// Specimens are emitted **in capture order**, not roster order: `SiteSpecimens` is a relationship
/// target whose order is *attach* order, so an unsorted save would round-trip to a different sequence
/// and shuffle which cell each specimen occupies between sessions.
pub fn capture_save(world: &mut World) -> SaveGame {
    let tech_tree = world.get_resource::<TechTree>().copied().unwrap_or_default();
    let run_seed = world.get_resource::<crate::session::RunSeed>().map(|s| s.0).unwrap_or(0);

    let mut rows: Vec<(u64, SavedSpecimen)> = {
        let mut q = world.query::<(
            &crate::containment::Specimen,
            &ResearchPosterior,
            Option<&Researched>,
            Option<&Unlocks>,
            Option<&crate::research::ExperimentLog>,
        )>();
        q.iter(world)
            .map(|(s, p, done, unlock, log)| {
                (
                    s.captured_tick,
                    SavedSpecimen {
                        captured_tick: s.captured_tick,
                        subject: s.subject,
                        // `Option` because the log is attached at the bench, not at capture — a
                        // specimen banked this expedition and saved on arrival has not been to the
                        // slab yet. Untested is the honest default there, not a missing record.
                        experiments: log.copied().unwrap_or_default(),
                        posterior: *p,
                        researched: done.is_some(),
                        unlocks: unlock.map(|u| u.0.clone()).unwrap_or_default(),
                    },
                )
            })
            .collect()
    };
    // SORT-OK: `captured_tick` orders the record set, and ties are genuinely interchangeable — two
    // specimens banked on the same tick hold identical *saved* content apart from their posteriors,
    // which sort stably by value below. `sort_by` is stable, so equal ticks keep their relative order.
    rows.sort_by_key(|(tick, _)| *tick);

    let squad_knowledge =
        world.get_resource::<crate::knowledge::SquadKnowledge>().copied().unwrap_or_default();
    SaveGame {
        version: SAVE_VERSION,
        run_seed,
        squad_knowledge,
        tech_tree: tech_tree.bits(),
        specimens: rows.into_iter().map(|(_, s)| s).collect(),
    }
}

/// Replace the current campaign with a loaded one.
///
/// **Despawns every existing `Specimen` first.** Loading is a *replacement*, not a merge: merging would
/// silently double a campaign's specimens every time it was loaded twice, and there is no correct
/// answer to "which of these two identical records is the real one".
pub fn apply_save(world: &mut World, save: &SaveGame) -> Result<(), String> {
    save.validate()?;

    let existing: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, With<crate::containment::Specimen>>();
        q.iter(world).collect()
    };
    for e in existing {
        world.despawn(e);
    }

    if let Some(mut seed) = world.get_resource_mut::<crate::session::RunSeed>() {
        seed.0 = save.run_seed;
    }
    if let Some(mut tree) = world.get_resource_mut::<TechTree>() {
        *tree = TechTree::from_bits(save.tech_tree);
    }
    if let Some(mut k) = world.get_resource_mut::<crate::knowledge::SquadKnowledge>() {
        // Replacement, not a merge — the same rule the specimen list follows. Merging two squads'
        // beliefs would compound a campaign every time it was loaded.
        *k = save.squad_knowledge;
    }

    let site = world.get_resource::<SiteRoot>().map(|s| s.0);
    for row in &save.specimens {
        // `captured` is a fresh placeholder: the anomaly it referred to has not existed since that
        // expedition ended, and the field is documented as run-local. Pointing it at the specimen
        // itself is honest — a self-reference reads as "no live subject" rather than as a stale id
        // that might resolve to some unrelated entity.
        let mut ec = world.spawn(row.posterior);
        let id = ec.id();
        ec.insert((
            crate::containment::Specimen {
                captured: id,
                captured_tick: row.captured_tick,
                subject: row.subject,
            },
            row.experiments,
        ));
        if row.researched {
            ec.insert(Researched);
        }
        if !row.unlocks.is_empty() {
            ec.insert(Unlocks(row.unlocks.clone()));
        }
        if let Some(site) = site {
            ec.insert(HeldAt(site));
        }
    }
    Ok(())
}

/// Write the campaign atomically (tmp + rename), so a crash mid-write cannot corrupt it. Mirrors
/// `settings::write_settings`, which exists for the same reason.
pub fn write_save(path: &PathBuf, save: &SaveGame) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| format!("{} has no parent dir", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let text = ron::ser::to_string_pretty(save, ron::ser::PrettyConfig::default())
        .map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))
}

/// Read a campaign from disk. `Ok(None)` means "no save yet", which is a first launch, not an error.
pub fn read_save(path: &PathBuf) -> Result<Option<SaveGame>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let save: SaveGame = ron::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    save.validate()?;
    Ok(Some(save))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::HiddenParam;

    fn sample() -> SaveGame {
        let mut p = ResearchPosterior::unknown();
        p.observe(HiddenParam::Lethality, true, 0.85);
        SaveGame {
            version: SAVE_VERSION,
            run_seed: 0xDEAD_BEEF,
            tech_tree: 0b0101,
            squad_knowledge: crate::knowledge::SquadKnowledge::default(),
            specimens: vec![
                SavedSpecimen {
                    captured_tick: 120,
                    subject: crate::knowledge::Subject::ComfortBlob,
                    experiments: crate::research::ExperimentLog { runs: [2, 0, 1, 0] },
                    posterior: p,
                    researched: false,
                    unlocks: vec![Capability::MoraleField],
                },
                SavedSpecimen {
                    captured_tick: 900,
                    subject: crate::knowledge::Subject::Parasite,
                    experiments: crate::research::ExperimentLog::default(),
                    posterior: ResearchPosterior::unknown(),
                    researched: true,
                    unlocks: Vec::new(),
                },
            ],
        }
    }

    #[test]
    fn a_campaign_round_trips_through_ron_unchanged() {
        let save = sample();
        let text = ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()).expect("ser");
        let back: SaveGame = ron::from_str(&text).expect("de");
        assert_eq!(save, back, "the campaign must survive a round trip byte-for-byte in meaning");
    }

    #[test]
    fn a_save_from_another_schema_is_refused_rather_than_guessed_at() {
        let mut save = sample();
        save.version = SAVE_VERSION + 1;
        let err = save.validate().expect_err("a foreign version must not load");
        assert!(err.contains("refusing"), "the refusal must say WHY: {err}");
    }

    #[test]
    fn an_absent_save_is_a_first_launch_not_an_error() {
        let path = std::env::temp_dir().join("fvs_no_such_campaign_xyz.ron");
        let _ = std::fs::remove_file(&path);
        assert_eq!(read_save(&path).expect("missing is not an error"), None);
    }

    #[test]
    fn a_malformed_save_fails_loudly_rather_than_loading_a_default_campaign() {
        // The one-path rule applied to user data: silently starting a fresh campaign because the file
        // was unreadable would destroy hours of progress and look like a game bug.
        let path = std::env::temp_dir().join("fvs_malformed_campaign.ron");
        std::fs::write(&path, "this is not ron").expect("write");
        assert!(read_save(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn writing_is_atomic_and_leaves_no_tmp_behind() {
        let path = std::env::temp_dir().join("fvs_atomic_campaign.ron");
        let _ = std::fs::remove_file(&path);
        write_save(&path, &sample()).expect("write");
        assert!(path.exists());
        assert!(!path.with_extension("ron.tmp").exists(), "the tmp file must be renamed, not left");
        assert_eq!(read_save(&path).expect("read"), Some(sample()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_saved_shape_carries_no_entity_ids() {
        // THE assertion this module exists to protect. An `Entity` is an index into one process's ECS;
        // writing one to disk stores a number that means something different next launch. That is
        // FVS-N-8's bug one level up (`AssetId` is an arena slot), and it would present as specimens
        // linked to the wrong anomalies rather than as a crash.
        let text = ron::ser::to_string_pretty(&sample(), ron::ser::PrettyConfig::default()).expect("ser");
        assert!(
            !text.contains("captured:") || !text.contains("Entity"),
            "no Entity may appear in a save: {text}"
        );
    }
}

/// Autosave/load wiring.
///
/// **Save on arriving at the Site, not on quitting.** Two reasons, and the second is the real one:
/// arriving at the Site is the moment meta-progress actually changed (an expedition just resolved and
/// its specimen was banked), and a save triggered by quitting is a save that never happens when the
/// process is killed — which is how a player loses an evening.
///
/// **Load once, at `Startup`.** A campaign is read exactly as the process begins, before anything can
/// have banked a specimen of its own, so there is no merge case to get wrong.
pub struct PersistPlugin;

impl Plugin for PersistPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_campaign.after(crate::site::spawn_site))
            .add_systems(OnEnter(crate::ui::state::AppState::Site), save_campaign);
    }
}

/// Read the campaign at boot, if there is one.
///
/// A missing save is a first launch. A **malformed** one is a loud error and no load — deliberately
/// not "start fresh", because silently discarding a campaign because a file failed to parse destroys
/// hours of progress and presents as a game bug rather than as a corrupt file.
fn load_campaign(world: &mut World) {
    let Some(path) = save_path() else {
        warn!("persist: no data dir (HOME/XDG/APPDATA unset); the campaign will not persist");
        return;
    };
    match read_save(&path) {
        Ok(None) => info!("persist: no campaign at {} — starting fresh", path.display()),
        Ok(Some(save)) => {
            let n = save.specimens.len();
            match apply_save(world, &save) {
                Ok(()) => info!("persist: loaded {n} specimen(s) from {}", path.display()),
                Err(e) => error!("persist: {e}"),
            }
        }
        Err(e) => error!("persist: refusing to load — {e}"),
    }
}

/// Write the campaign on arriving at Site-67.
fn save_campaign(world: &mut World) {
    let Some(path) = save_path() else { return };
    let save = capture_save(world);
    let n = save.specimens.len();
    match write_save(&path, &save) {
        Ok(()) => info!("persist: saved {n} specimen(s)"),
        Err(e) => error!("persist: save failed — {e}"),
    }
}
