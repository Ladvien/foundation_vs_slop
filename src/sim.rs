//! Hot-tunable numeric knobs for the **simulation dynamics** — combat, the swarm economy, field-deposit
//! strengths, fear gains, and the boss — deserialized from the `sim:` slice of the unified
//! `assets/config/config.ron` at startup (loaded + validated once by [`crate::config::ConfigPlugin`]).
//! Required config — one path, no fallback: a missing or malformed slice is a loud failure at the loader.
//!
//! This mirrors [`crate::ai::tuning::AiTuning`] (which owns the field *propagation* knobs — evaporate /
//! diffuse / radius). Together `AiTuning` + `SimTuning` are the full data-driven surface an offline search
//! evolves as a `WorldConfig` (see `squad_ai::world_genome`). Structure stays in code (systems, factions,
//! channels are type-safe Rust); only the *numbers* live here, so a designer — or the search — can retune
//! world dynamics and relaunch without recompiling.
//!
//! Every value in [`SimTuning::default`] is **bit-identical** to the Rust `const` it replaced; a
//! `sim_default_equals_shipped_config` test pins that the RON slice matches the default, and the
//! deterministic-core replay hash pins that promoting these consts changed no gameplay math.

use serde::{Deserialize, Serialize};

/// Fear gains — how strongly a drive tracks an enemy's threat channel. Each threat channel is laid at
/// ≈ its own evaporation rate, so a cell's value tracks the local emitter *count*, and the gain reads as
/// "fear per emitter". `Flee` needs FEAR ≳ 0.28 to clear `MIN_SCORE`, so `per_crab = 0.08` holds the squad
/// against one or two crabs and breaks it under four — a firefight, not a rout. `of_anomaly` is near-total:
/// standing in the watcher's aura is meant to rout the squad. Nothing may fear a channel it emits.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FearTuning {
    /// A unit's fear per nearby crab (tracks `THREAT_CRAB`).
    pub per_crab: f32,
    /// A unit's fear of the watcher (tracks `THREAT_ANOMALY`).
    pub of_anomaly: f32,
    /// A crab's fear of the squad's gunfire (tracks `THREAT_GUN`).
    pub crab_of_gunfire: f32,
}

/// Stigmergy deposit *strengths* (the amount laid per event/second). The paired evaporate/diffuse/radius
/// for each channel live in [`crate::ai::tuning::AiTuning`]; several channels are designed so
/// `deposit ≈ evaporate` (a cell reads as a "count"), so evolving one without the other shifts semantics —
/// that is why both sides are promoted together.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepositTuning {
    /// `THREAT_GUN` laid at muzzle + impact per shot.
    pub threat_per_shot: f32,
    /// Blood scent laid into `SCENT` at a death (crab or boss).
    pub blood_scent: f32,
    /// `CRAB_DENSITY` laid per crab per second (the "reads-as-a-count" rate).
    pub crab_density_rate: f32,
    /// `THREAT_CRAB` (dread) laid per crab per second.
    pub crab_menace_rate: f32,
    /// `MEAT` laid per fruit/carrion source per second.
    pub meat_rate: f32,
    /// `THREAT_ANOMALY` aura laid by the living watcher per second.
    pub anomaly_aura_rate: f32,
    /// `THREAT_ANOMALY` dread laid per **roused** SCP-150 manca per second. Distinct from
    /// `anomaly_aura_rate` (the single watcher's broad standing aura): a roused brood is many small
    /// emitters clustered together, so the per-capita rate is deliberately lower — the swarm's felt dread
    /// comes from overlap/clustering, not from each manca out-dreading the god. Dormant mancae emit nothing.
    pub manca_dread_rate: f32,
    /// `ALARM` flooded around a freshly wounded crab.
    pub alarm_crab: f32,
    /// `ALARM` flooded around a wounded nest.
    pub alarm_nest: f32,
    /// Rally-vector strength a scout deposits toward live prey.
    pub rally_mark: f32,
}

/// Combat numbers — weapon damage, the crab bite, and hit points. `crab_damage_exponent` makes a pile-on
/// super-linear (`dps · count^exp`), so being swarmed is the real threat, not a single bite.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CombatTuning {
    /// Damage per laser hit.
    pub laser_damage: f32,
    /// Chance a laser that misses its target strikes a unit in the friendly arc.
    pub friendly_fire_chance: f32,
    /// Damage a friendly-fire hit deals.
    pub friendly_fire_damage: f32,
    /// Per-crab contact damage-per-second (the bite base).
    pub crab_contact_dps: f32,
    /// Super-linear exponent on the biting-crab count.
    pub crab_damage_exponent: f32,
    /// Damage a crab's pounce/jump bite deals.
    pub crab_jump_damage: f32,
    /// A crab's hit points.
    pub crab_hp: f32,
    /// A squad unit's hit points.
    pub unit_hp: f32,
    /// Speed drag a unit suffers per crab clinging to it (`speed / (1 + crabs · drag)`).
    pub crab_drag: f32,
}

/// Swarm economy — breeding, feeding. No population cap and no local crowding gate: the meat economy
/// (`meat_per_crab` vs. hoard) is the swarm's only size lever, so a well-fed nest breeds without limit.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreedingTuning {
    /// Minimum seconds between a nest's births (breed rate limiter).
    pub respawn_interval: f32,
    /// Meat consumed per birth.
    pub meat_per_crab: f32,
    /// How strongly delivered meat boosts a nest's spawn drive.
    pub feed_gain: f32,
    /// Ceiling on the accumulated spawn boost.
    pub spawn_boost_max: f32,
    /// Per-second decay of the spawn boost.
    pub spawn_boost_decay: f32,
    /// Per-second rise of a crab's HUNGER drive (pushes foraging/feeding).
    pub hunger_rate: f32,
    /// Per-second drain of HUNGER while feeding.
    pub hunger_sate_rate: f32,
}

/// The watcher (boss). `start_hp` is the dominant fight-length lever. The `cull_*` knobs govern how it
/// swats biting crabs off itself. The vestigial `CONTACT_DPS` (a death-camera mass weight, never applied
/// as damage) is deliberately **not** promoted — it is not a dynamics knob.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BossTuning {
    /// The watcher's hit points.
    pub start_hp: f32,
    /// Seconds the watcher recoils/flees after being hit.
    pub scared_time: f32,
    /// Minimum seconds between the watcher's lightning zaps.
    pub zap_cadence: f32,
    /// Biting crabs on the watcher before it swats.
    pub cull_threshold: usize,
    /// Crabs within this radius of the watcher's centre are eaten by a swat.
    pub cull_radius: f32,
    /// Most crabs one swat removes (bounds the swarm hit).
    pub cull_max: usize,
    /// Seconds between swats.
    pub cull_cooldown: f32,
}

/// SCP-150 parasite dynamics — the free manca's population/combat/gait knobs, the gestation clock, the
/// brood size, and the host-manipulation strengths. Geometry/render constants (collider radius, model
/// scale/seat) stay as code consts in `crate::parasite`; only the gameplay-dynamics numbers live here, so
/// the offline search can evolve the parasite like the crab swarm. `manca_count_max` is the load-bearing
/// cap that keeps the burst→brood→infest loop from exploding.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParasiteTuning {
    /// Free mancae seeded at level start.
    pub initial_count: usize,
    /// Hard cap on live mancae (bounds the reproduction loop).
    pub manca_count_max: usize,
    /// A manca's hit points.
    pub manca_hp: f32,
    /// Free-crawl / stalk speed on the floor (world units/s).
    pub crawl_speed: f32,
    /// Wall-climb speed (world units/s).
    pub climb_speed: f32,
    /// Ballistic-leap reach: a manca lunges at a host within this planar distance.
    pub leap_len: f32,
    /// Seconds between a manca's leaps.
    pub leap_cooldown: f32,
    /// Bite damage dealt to the host at the moment of burrowing in.
    pub embed_damage: f32,
    /// Seconds a parasite gestates inside a host before it bursts out.
    pub gestation_seconds: f32,
    /// Brood size on burst is drawn from `[brood_min, brood_max]`.
    pub brood_min: u32,
    pub brood_max: u32,
    /// Host manipulation: the COHESION an infested unit is forced to (low ⇒ stops rejoining the squad).
    pub manip_cohesion_drop: f32,
    /// Host manipulation: the CURIOSITY an infested unit is forced to (high ⇒ wanders off).
    pub manip_curiosity_gain: f32,
    /// Host manipulation: how far (world units) an infested unit's goal is pushed down the light gradient
    /// toward shadow.
    pub manip_dark_gain: f32,
}

/// SCP-999 — the friendly "Tickle Monster" comfort blob (see `crate::scp999`). The only creature that
/// *lowers* squad anxiety: it oozes to the most-frightened member and, on contact, drains their FEAR and
/// lifts MORALE (companion-animal social buffering — Beetz et al. 2012). Every dial is evolvable
/// (`squad_ai::world_genome`) — including `count` — so the ecosystem search can tune both how strongly a
/// comfort source counteracts fear and how many are present (POET-style world co-evolution).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scp999Tuning {
    /// Comfort blobs seeded into the level at start.
    pub count: usize,
    /// Ooze speed toward the most-anxious member (world units/s).
    pub move_speed: f32,
    /// Extra reach beyond the member's body radius at which a touch counts as a tickle.
    pub contact_radius: f32,
    /// FEAR drained per second while tickling a member (the anxiety relief).
    pub calm_rate: f32,
    /// MORALE lifted per second while tickling a member.
    pub morale_rate: f32,
    /// How far from the squad's spawn cell (in tiles) a blob must start — it seeds out in the level like
    /// the crabs and the Smiley, not at the squad's feet, so relief has to be *found*. This is the lever
    /// that sets how long the squad carries its fear before the comfort source reaches it, which is why it
    /// evolves alongside `calm_rate` rather than sitting as a code constant.
    pub spawn_min_dist: f32,
}

/// SCP-1048 "Builder Bear" — the benign original and its three hostile copies (see `crate::scp1048`).
///
/// One slice covers all four variants: they share a rig, a movement model and a strike cadence, and
/// differ only in *attack expression* (A shrieks, B throws a tantrum, C fires a scrap gun), which is
/// a code branch rather than a number. The interesting evolvable loop is that the **original builds
/// the copies**: `scavenge_rate`/`build_cost`/`build_cooldown` set how fast a hostile population
/// appears, and `max_bears` is the firm cap that keeps it from exploding — exactly the role
/// `parasite.manca_count_max` plays for the burst→brood→infest loop.
///
/// Two mechanics carry academic grounding, cited where they are implemented:
/// - **Build only while unobserved** — stigmergic construction, where local deposition is amplified
///   by the state of the structure rather than centrally planned (Khuong et al., "Stigmergic
///   construction and topochemical information shape ant nest architecture", PNAS 2016,
///   doi:10.1073/pnas.1509829113).
/// - **The ear-growth DoT** — damage that accumulates under exposure, is repaired at a constant rate
///   once exposure ends, and kills only above a threshold: the General Unified Threshold model of
///   Survival (Jager, Albert, Preuss & Ashauer, Environ. Sci. Technol. 2011, doi:10.1021/es103092a).
///   `growth_rate` is the accumulation term, `growth_decay` the repair term, `asphyxiate_threshold`
///   the threshold. A `growth_decay` of 0 is a deliberately reachable world: incurable growths.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scp1048Tuning {
    /// Benign originals seeded into the level at start. The hostile copies are *built*, not seeded.
    pub count: usize,
    /// How far from the squad's spawn cell (in tiles) a bear must start.
    pub spawn_min_dist: f32,
    /// Shuffle speed (world units/s) — a 0.33 m plush is slow.
    pub move_speed: f32,
    /// Hit points. The original is unshootable (no `Hostile`), but still carries `Health` so the
    /// deterministic-core snapshot folds it.
    pub hp: f32,
    /// How close a hostile copy closes before it can strike.
    pub approach_range: f32,
    /// Attack reach.
    pub strike_range: f32,
    /// Seconds between strikes; doubles as C's rate of fire.
    pub strike_cooldown: f32,
    /// Damage per strike (B's tantrum, C's shot and pistol-whip).
    pub strike_damage: f32,
    /// THREAT_ANOMALY deposited per second by a *raging* copy. A wandering copy deposits nothing.
    pub rage_dread_rate: f32,
    /// One-shot dread burst stamped when SCP-1048-A screams.
    pub scream_dread: f32,
    /// Radius of the scream's pain/dread band (canon: ~10 m of blinding pain).
    pub pain_radius: f32,
    /// Radius of the scream's lethal ear-growth band (canon: ~5 m). Smaller than `pain_radius`: the
    /// shriek terrifies further than it kills.
    pub growth_radius: f32,
    /// Ear-growth severity accrued per second inside `growth_radius` (canon: full cover in ~20 s).
    pub growth_rate: f32,
    /// Severity shed per second outside it — the GUTS repair term. 0 ⇒ incurable.
    pub growth_decay: f32,
    /// HP/s once severity is at or past the threshold (canon: asphyxiation within ~3 min).
    pub asphyxiate_dps: f32,
    /// Severity at which the growths start to suffocate.
    pub asphyxiate_threshold: f32,
    /// Bear FEAR gain on the THREAT_GUN channel — how badly gunfire panics a bear.
    pub fear_of_gunfire: f32,
    /// Scavenged material one copy costs to build.
    pub build_cost: f32,
    /// Material accrued per second while the original is building.
    pub scavenge_rate: f32,
    /// Seconds after a build before the next may start.
    pub build_cooldown: f32,
    /// Firm cap on live bears of all variants — the load-bearing bound on the replication loop.
    pub max_bears: usize,
    /// Relative weight of building an A (ear) copy.
    pub copy_w_a: f32,
    /// Relative weight of a B (infant-arm) copy; C takes `max(0, 1 - copy_w_a - copy_w_b)`. The
    /// three always sum to >= 1, so the draw needs no clamp and cannot divide by zero.
    pub copy_w_b: f32,
}

/// Root simulation-tuning resource. Extend with new sections as later phases need them; keep
/// [`SimTuning::default`] bit-identical to the shipped consts, guarded by the deterministic-core hash.
#[derive(bevy::prelude::Resource, Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimTuning {
    pub fear: FearTuning,
    pub deposit: DepositTuning,
    pub combat: CombatTuning,
    pub breeding: BreedingTuning,
    pub boss: BossTuning,
    pub parasite: ParasiteTuning,
    pub scp999: Scp999Tuning,
    pub scp1048: Scp1048Tuning,
    pub containment: ContainmentTuning,
    /// **How hard an operative's beliefs bite** (FVS-O-2). Scales the FEAR of an operative who is near
    /// a subject they hold a confident `Lethal` belief about; `0.0` disables the coupling entirely and
    /// is a *bit-exact* no-op, which is how it ships (see `knowledge::coupling`).
    ///
    /// Evolvable, and it belongs here rather than in a rules slice for the reason
    /// `ContainmentTuning` records: this is **difficulty**, which is exactly what the world genome
    /// exists to explore. What a belief *means* is not evolvable; how much it costs you is.
    pub belief_fear_gain: f32,
}

/// **Containment LOGISTICS** — how many devices the squad carries, how far each verb reaches, how big
/// the extraction zone is.
///
/// Deliberately here, on `SimTuning`, and *not* in the `containment:` config slice, because the two
/// answer different questions and only one of them may evolve:
///
/// * The `containment:` slice holds the **rules** — what containing an anomaly *means*. A search free
///   to retune a rule would be moving the objective rather than solving it, which is the same reasoned
///   exception `session::SessionConfig` documents for the win condition.
/// * These are **difficulty**. How many canisters an expedition gets is exactly the kind of knob the
///   world genome exists to explore, so it belongs on the evolvable side.
///
/// Living on `SimTuning` is also what keeps the wiring cheap: `coevolve::artifacts::WorldEliteDoc`
/// already carries `pub sim: SimTuning` and `elite_overlay::apply_dim`'s `Dim::World` arm already does
/// `gc.sim = e.sim`, so only `world_genome`'s BOUNDS/encode/decode need to learn these. That is the
/// payoff `config::WorldConfig`'s doc warns about missing — a new top-level slice would have had to be
/// threaded through all four sites, and the doc records that mould and almond water were once exactly
/// that mistake for 23 of 102 knobs.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainmentTuning {
    /// Capture devices the squad carries into an expedition. A miss spends one.
    pub device_supply: u32,
    /// How far from the thrower a device may connect with its named target (world units).
    pub device_reach: f32,
    /// Quarantine regions the squad may deploy. Deliberately a *separate* pool from `device_supply`:
    /// the three archetypes are genuinely distinct verbs, and making their charges fungible would let
    /// the player collapse them back into one.
    pub quarantine_supply: u32,
    /// Radius of a deployed quarantine region (world units).
    pub quarantine_radius: f32,
    /// How close a unit must be to a nest to cap it (world units).
    pub cap_reach: f32,
    /// Radius of the run's extraction zone at the insertion cell (world units).
    pub extraction_radius: f32,
    /// Ambient `ATTENTION` at which an anomaly counts as **out-watched** (FVS-C-3).
    ///
    /// One knob feeding two places on purpose: it gates SCP-1048's scavenging *and* is the threshold
    /// its authored containment rule uses. Suppressing the build and completing the capture are then
    /// literally the same action, and the player never has to learn two numbers.
    pub out_watch_threshold: f32,
}

impl Default for SimTuning {
    fn default() -> Self {
        Self {
            // FVS-O-2 ships OFF. `0.0` makes `knowledge::coupling::apply_belief_fear` a bit-exact
            // no-op, so the goldens do not move for a mechanic nobody enabled; turning it on is a
            // deliberate act that earns its own measured re-pin.
            belief_fear_gain: 0.4,
            fear: FearTuning {
                per_crab: 0.08,
                of_anomaly: 0.9,
                crab_of_gunfire: 0.2,
            },
            deposit: DepositTuning {
                threat_per_shot: 0.6,
                blood_scent: 4.0,
                crab_density_rate: 0.4,
                crab_menace_rate: 0.5,
                meat_rate: 0.5,
                anomaly_aura_rate: 0.4,
                manca_dread_rate: 0.1,
                alarm_crab: 2.0,
                alarm_nest: 4.0,
                rally_mark: 4.0,
            },
            combat: CombatTuning {
                laser_damage: 10.0,
                friendly_fire_chance: 0.2,
                friendly_fire_damage: 5.0,
                crab_contact_dps: 2.3,
                crab_damage_exponent: 1.5,
                crab_jump_damage: 8.0,
                crab_hp: 25.0,
                unit_hp: 100.0,
                crab_drag: 0.15,
            },
            breeding: BreedingTuning {
                respawn_interval: 5.0,
                meat_per_crab: 1.0,
                feed_gain: 6.0,
                spawn_boost_max: 9.0,
                spawn_boost_decay: 1.0,
                hunger_rate: 0.03,
                hunger_sate_rate: 0.3,
            },
            boss: BossTuning {
                start_hp: 2400.0,
                scared_time: 1.6,
                zap_cadence: 0.35,
                cull_threshold: 4,
                cull_radius: 1.4,
                cull_max: 6,
                cull_cooldown: 2.0,
            },
            parasite: ParasiteTuning {
                initial_count: 6,
                manca_count_max: 20,
                manca_hp: 18.0,
                crawl_speed: 1.8,
                climb_speed: 1.8,
                leap_len: 1.9,
                leap_cooldown: 2.5,
                embed_damage: 12.0,
                gestation_seconds: 120.0,
                brood_min: 2,
                brood_max: 3,
                manip_cohesion_drop: 0.05,
                manip_curiosity_gain: 0.9,
                manip_dark_gain: 3.0,
            },
            scp999: Scp999Tuning {
                count: 1,
                move_speed: 2.2,      // a touch faster than the crab crawl (1.8) so it can reach members
                contact_radius: 0.9,  // reach = UNIT_BODY_RADIUS(0.33) + 0.9 = ~1.23 m; a generous tickle
                calm_rate: 0.6,       // ~1.6 s of contact to soothe FEAR 1→0 — a gentle sustained comfort
                morale_rate: 0.4,
                spawn_min_dist: 18.0, // well past the crabs' 12 — the comfort blob is the far reward
            },
            scp1048: Scp1048Tuning {
                count: 1,
                spawn_min_dist: 16.0, // out in the level like the blob; the bear has to be found
                move_speed: 0.9,      // half the crab crawl (1.8) — a 33 cm plush shuffles
                hp: 30.0,             // sturdier than a manca (18), far softer than the boss
                approach_range: 6.0,
                strike_range: 1.2,       // ~the crab's latch reach
                strike_cooldown: 1.5,
                strike_damage: 8.0,      // parity with combat.crab_jump_damage
                rage_dread_rate: 0.3,    // 3x manca_dread_rate — a raging bear is loud on the field
                scream_dread: 4.0,       // parity with deposit.alarm_nest: one shriek reads as an alarm
                pain_radius: 10.0,       // canon: blinding pain at 10 m
                growth_radius: 5.0,      // canon: ear growths at 5 m
                growth_rate: 0.05,       // ~20 s inside the band to full cover
                growth_decay: 0.01,      // leaving the band saves you: 40 s from full back under the
                                         //   threshold, so a rescued victim suffocates a while, then lives
                asphyxiate_dps: 0.6,     // ~100 HP over ~170 s — canon's "within 3 minutes"
                // MUST stay < 1.0. At 1.0 the lethal band is the single point at the top of the
                // severity range, so `growth_decay` drops a victim under it on the very first tick
                // after the shriek stops and the suffocation can only ever tick while the bear is
                // actively screaming — the canon "died within 3 minutes" could never happen.
                asphyxiate_threshold: 0.6,
                fear_of_gunfire: 0.2,    // parity with fear.crab_of_gunfire
                build_cost: 12.0,
                scavenge_rate: 1.0,      // 12 s of unobserved building per copy
                build_cooldown: 20.0,
                max_bears: 6,
                copy_w_a: 0.34, // the three copies are near-equally likely; C takes the remainder
                copy_w_b: 0.33,
            },
            containment: ContainmentTuning {
                device_supply: 3,     // enough to miss twice and still make the tutorial capture
                device_reach: 2.5,    // ~2.5 tiles: a deliberate approach, not a cross-room snipe
                quarantine_supply: 1, // the area verb is the scarce one — it bounds a whole region
                quarantine_radius: 3.0,
                cap_reach: 1.5,       // parity with the crab latch reach: you seal a nest at arm's length
                extraction_radius: 2.5, // wide enough that five units fit without shoving each other out
                out_watch_threshold: 0.45, // matches the authored scp1048 rule in config.ron
            },
        }
    }
}

/// Range-check every knob. One path, no fallback: an out-of-range value is a loud `Err` the loader
/// surfaces (`load_game_config`), never a silent clamp. The bounds are physical-plausibility gates that
/// the shipped defaults sit comfortably inside; the offline search's `WorldGenome` bounds table is tighter
/// still (it also caps the *upper* end per knob to keep worlds playable).
pub fn validate_tuning(t: &SimTuning) -> Result<(), String> {
    let positive = |name: &str, v: f32| -> Result<(), String> {
        if v > 0.0 && v.is_finite() {
            Ok(())
        } else {
            Err(format!("sim tuning: {name} must be finite and > 0 (got {v})"))
        }
    };
    let non_negative = |name: &str, v: f32| -> Result<(), String> {
        if v >= 0.0 && v.is_finite() {
            Ok(())
        } else {
            Err(format!("sim tuning: {name} must be finite and >= 0 (got {v})"))
        }
    };
    let probability = |name: &str, v: f32| -> Result<(), String> {
        if (0.0..=1.0).contains(&v) {
            Ok(())
        } else {
            Err(format!("sim tuning: {name} must be a probability in [0,1] (got {v})"))
        }
    };

    // Fear gains are positive multipliers on a threat sample.
    positive("fear.per_crab", t.fear.per_crab)?;
    positive("fear.of_anomaly", t.fear.of_anomaly)?;
    positive("fear.crab_of_gunfire", t.fear.crab_of_gunfire)?;

    // Deposit strengths are positive amounts.
    positive("deposit.threat_per_shot", t.deposit.threat_per_shot)?;
    positive("deposit.blood_scent", t.deposit.blood_scent)?;
    positive("deposit.crab_density_rate", t.deposit.crab_density_rate)?;
    positive("deposit.crab_menace_rate", t.deposit.crab_menace_rate)?;
    positive("deposit.meat_rate", t.deposit.meat_rate)?;
    positive("deposit.anomaly_aura_rate", t.deposit.anomaly_aura_rate)?;
    positive("deposit.manca_dread_rate", t.deposit.manca_dread_rate)?;
    positive("deposit.alarm_crab", t.deposit.alarm_crab)?;
    positive("deposit.alarm_nest", t.deposit.alarm_nest)?;
    positive("deposit.rally_mark", t.deposit.rally_mark)?;

    // Combat.
    positive("combat.laser_damage", t.combat.laser_damage)?;
    probability("combat.friendly_fire_chance", t.combat.friendly_fire_chance)?;
    non_negative("combat.friendly_fire_damage", t.combat.friendly_fire_damage)?;
    positive("combat.crab_contact_dps", t.combat.crab_contact_dps)?;
    if !(t.combat.crab_damage_exponent >= 1.0 && t.combat.crab_damage_exponent.is_finite()) {
        return Err(format!(
            "sim tuning: combat.crab_damage_exponent must be finite and >= 1 (got {})",
            t.combat.crab_damage_exponent
        ));
    }
    positive("combat.crab_jump_damage", t.combat.crab_jump_damage)?;
    positive("combat.crab_hp", t.combat.crab_hp)?;
    positive("combat.unit_hp", t.combat.unit_hp)?;
    non_negative("combat.crab_drag", t.combat.crab_drag)?;

    // Breeding.
    positive("breeding.respawn_interval", t.breeding.respawn_interval)?;
    positive("breeding.meat_per_crab", t.breeding.meat_per_crab)?;
    positive("breeding.feed_gain", t.breeding.feed_gain)?;
    positive("breeding.spawn_boost_max", t.breeding.spawn_boost_max)?;
    positive("breeding.spawn_boost_decay", t.breeding.spawn_boost_decay)?;
    positive("breeding.hunger_rate", t.breeding.hunger_rate)?;
    positive("breeding.hunger_sate_rate", t.breeding.hunger_sate_rate)?;

    // Boss.
    positive("boss.start_hp", t.boss.start_hp)?;
    positive("boss.scared_time", t.boss.scared_time)?;
    positive("boss.zap_cadence", t.boss.zap_cadence)?;
    if t.boss.cull_threshold == 0 {
        return Err("sim tuning: boss.cull_threshold must be >= 1".into());
    }
    positive("boss.cull_radius", t.boss.cull_radius)?;
    if t.boss.cull_max == 0 {
        return Err("sim tuning: boss.cull_max must be >= 1".into());
    }
    positive("boss.cull_cooldown", t.boss.cull_cooldown)?;

    // Parasite (SCP-150).
    if t.parasite.manca_count_max == 0 {
        return Err("sim tuning: parasite.manca_count_max must be >= 1".into());
    }
    positive("parasite.manca_hp", t.parasite.manca_hp)?;
    positive("parasite.crawl_speed", t.parasite.crawl_speed)?;
    positive("parasite.climb_speed", t.parasite.climb_speed)?;
    positive("parasite.leap_len", t.parasite.leap_len)?;
    positive("parasite.leap_cooldown", t.parasite.leap_cooldown)?;
    non_negative("parasite.embed_damage", t.parasite.embed_damage)?;
    positive("parasite.gestation_seconds", t.parasite.gestation_seconds)?;
    if !(t.parasite.brood_min >= 1 && t.parasite.brood_max >= t.parasite.brood_min) {
        return Err(format!(
            "sim tuning: parasite brood must satisfy 1 <= brood_min <= brood_max (got {} .. {})",
            t.parasite.brood_min, t.parasite.brood_max
        ));
    }
    probability("parasite.manip_cohesion_drop", t.parasite.manip_cohesion_drop)?;
    probability("parasite.manip_curiosity_gain", t.parasite.manip_curiosity_gain)?;
    positive("parasite.manip_dark_gain", t.parasite.manip_dark_gain)?;

    // SCP-999 (comfort blob). `count` is a usize (0 = a world with no comfort blob, a valid scenario), so
    // only the continuous effect/movement dials are range-checked here.
    positive("scp999.move_speed", t.scp999.move_speed)?;
    positive("scp999.contact_radius", t.scp999.contact_radius)?;
    positive("scp999.calm_rate", t.scp999.calm_rate)?;
    positive("scp999.morale_rate", t.scp999.morale_rate)?;
    positive("scp999.spawn_min_dist", t.scp999.spawn_min_dist)?;

    // SCP-1048 (the Builder Bear family). `count` is a usize (0 = a world with no bear at all, a
    // valid scenario), but `max_bears` is the cap the replication loop breaks on, so 0 there would
    // be a world that can never hold the bear it seeded — rejected, exactly as `manca_count_max` is.
    //
    // Four knobs are deliberately `non_negative` rather than `positive`, because their genome BOUNDS
    // floor is 0.0 and a validator stricter than BOUNDS would make mutated-but-in-bounds children
    // infeasible (which `world_genome::mutation_stays_within_bounds_and_finite` would red):
    //   - `growth_decay` 0 ⇒ ear growths never heal (an incurable world, reachable on purpose)
    //   - `rage_dread_rate` / `scream_dread` 0 ⇒ a bear that terrifies nobody
    //   - `strike_damage` 0 ⇒ copies that menace but cannot kill
    // No cross-knob constraint ties `growth_radius` to `pain_radius`: a world where the lethal band
    // out-reaches the pain band is strange but playable, and gating it would reject valid mutants.
    positive("scp1048.spawn_min_dist", t.scp1048.spawn_min_dist)?;
    positive("scp1048.move_speed", t.scp1048.move_speed)?;
    positive("scp1048.hp", t.scp1048.hp)?;
    positive("scp1048.approach_range", t.scp1048.approach_range)?;
    positive("scp1048.strike_range", t.scp1048.strike_range)?;
    positive("scp1048.strike_cooldown", t.scp1048.strike_cooldown)?;
    non_negative("scp1048.strike_damage", t.scp1048.strike_damage)?;
    non_negative("scp1048.rage_dread_rate", t.scp1048.rage_dread_rate)?;
    non_negative("scp1048.scream_dread", t.scp1048.scream_dread)?;
    positive("scp1048.pain_radius", t.scp1048.pain_radius)?;
    positive("scp1048.growth_radius", t.scp1048.growth_radius)?;
    positive("scp1048.growth_rate", t.scp1048.growth_rate)?;
    non_negative("scp1048.growth_decay", t.scp1048.growth_decay)?;
    positive("scp1048.asphyxiate_dps", t.scp1048.asphyxiate_dps)?;
    positive("scp1048.asphyxiate_threshold", t.scp1048.asphyxiate_threshold)?;
    positive("scp1048.fear_of_gunfire", t.scp1048.fear_of_gunfire)?;
    positive("scp1048.build_cost", t.scp1048.build_cost)?;
    positive("scp1048.scavenge_rate", t.scp1048.scavenge_rate)?;
    positive("scp1048.build_cooldown", t.scp1048.build_cooldown)?;
    if t.scp1048.max_bears == 0 {
        return Err("sim tuning: scp1048.max_bears must be >= 1".into());
    }
    probability("scp1048.copy_w_a", t.scp1048.copy_w_a)?;
    probability("scp1048.copy_w_b", t.scp1048.copy_w_b)?;

    // Containment logistics. Reaches and radii are positive distances; the two supplies may be zero
    // (an expedition authored without a given verb is a legitimate difficulty setting, unlike a rule
    // with no clauses — which `ContainmentRule::validate` rejects, because THAT would be an anomaly
    // that captures itself).
    positive("containment.device_reach", t.containment.device_reach)?;
    positive("containment.quarantine_radius", t.containment.quarantine_radius)?;
    positive("containment.cap_reach", t.containment.cap_reach)?;
    positive("containment.extraction_radius", t.containment.extraction_radius)?;
    probability("containment.out_watch_threshold", t.containment.out_watch_threshold)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_defaults_validate() {
        assert!(validate_tuning(&SimTuning::default()).is_ok());
    }

    #[test]
    fn sim_default_equals_shipped_config() {
        // The byte-identity guard for the const→config migration: the `sim:` slice in the shipped
        // `config.ron` must equal `SimTuning::default()` exactly. A transcription typo in the RON reds this
        // test instead of silently shifting a gameplay value (and the deterministic-core replay hash). We do
        // NOT use `#[serde(default)]` on the slice, precisely so a missing/renamed field is a loud parse
        // error here rather than a silent fallback (one path, no fallback).
        let cfg = crate::config::load_game_config().expect("shipped game config must load");
        assert_eq!(
            cfg.sim,
            SimTuning::default(),
            "assets/config/config.ron `sim:` slice drifted from the shipped SimTuning defaults"
        );
    }

    #[test]
    fn validator_rejects_out_of_range() {
        let mut t = SimTuning::default();
        t.combat.friendly_fire_chance = 1.5;
        assert!(validate_tuning(&t).is_err(), "a >1 probability must be rejected");

        let mut t = SimTuning::default();
        t.combat.crab_damage_exponent = 0.5;
        assert!(validate_tuning(&t).is_err(), "an exponent < 1 must be rejected");

        let mut t = SimTuning::default();
        t.breeding.respawn_interval = 0.0;
        assert!(validate_tuning(&t).is_err(), "a non-positive respawn interval must be rejected");

        let mut t = SimTuning::default();
        t.fear.per_crab = 0.0;
        assert!(validate_tuning(&t).is_err(), "a non-positive fear gain must be rejected");
    }
}
