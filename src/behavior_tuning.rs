//! Data-driven **creature/squad behaviour** tuning, deserialized from the `behavior:` slice of the
//! unified `assets/config/config.ron` at startup (loaded + validated once by `crate::config::ConfigPlugin`).
//! Required config — one path, no fallback: a missing or malformed field is a loud failure at the loader.
//!
//! This slice lifts the per-agent *behavioural* constants that used to live at the top of each system
//! file (`ai/brain.rs`, `squad_ai/perception.rs`, `squad.rs`, `enemy.rs`, `laser.rs`, `crab.rs`,
//! `parasite.rs`, `mycelia/grazing.rs`, `mycelia/control.rs`) into one hand-tunable place, so a designer
//! can retune emergence without recompiling and the offline behaviour search (`squad_ai::behavior_genome`)
//! can evolve a chosen subset. Only *behavioural* numbers move here — decision gates, sight ranges,
//! locomotion speeds, steering weights, cooldowns, boids parameters. **Structural** constants stay in
//! code: render geometry (`*_RENDER_SCALE`, capsule dims, gun/muzzle offsets), grid resolutions, spawn
//! counts / layout, hash seeds, enum counts, and VFX-only durations.
//!
//! Kept deliberately out of this slice (already owned elsewhere, no double-search): field propagation
//! (`ai_tuning`), the world-dynamics genome surface (`sim`: combat/breeding/boss-hp/parasite lethality),
//! lighting, mycelia growth clock, acoustic stimulus (`audio`). This slice is a *separate* genome surface
//! (`behavior_genome`), so growing it never shifts the frozen `world_genome` encoding.
//!
//! All continuous fields are `Copy` + `Serialize`, so an evolved value decodes to a readable RON diff —
//! the reward-hacking guard (Skalse et al., "Defining and Characterizing Reward Hacking", arXiv:2209.13085),
//! mirroring [`crate::config::WorldConfig`] and [`crate::audio_tuning::AudioTuning`].

use serde::{Deserialize, Serialize};

// NOTE — the creature *decision-core* gates (`HUNT_SCENT_MIN`, `RALLY_MIN`, `ALARM_MIN`, `THINK_INTERVAL`,
// `CHASE_TILES` in `ai/brain.rs`; `TRACK_EASE` in `ai/drives.rs`; `MIN_SCORE` in `ai/utility.rs`;
// `ATTENTION_RATE` in `ai/field.rs`) are deliberately NOT lifted here. They live in the authored utility
// repertoire (baked into the brain data literals `smiley_brain`/`crab_brain`/`scout_brain`, which the
// `squad_ai::genome` BRAIN search already evolves) and in the determinism-critical decision core
// (`utility::decide`, drive tracking). Externalising them would thread config through the hot decision
// path AND create two sources for one gate (config value vs the brain literal) — the "two paths" hazard.
// This slice is scoped to *physical* behaviour: senses, locomotion, steering, combat cadence, boids.

/// Squad perception sight ranges + Schmitt-trigger hysteresis bands (`squad_ai/perception.rs`,
/// `squad_ai/role.rs`). Each `*_sight` is the trigger radius; `*_release` is the (larger) let-go radius.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct PerceptionTuning {
    pub examine_sight: f32,
    pub examine_sight_release: f32,
    pub threat_sight: f32,
    pub threat_sight_release: f32,
    pub psi_sight: f32,
    pub psi_sight_release: f32,
    /// Researcher flashlight ward-of sight / release.
    pub ward_sight: f32,
    pub ward_sight_release: f32,
    /// Medic trigger: health fraction below which a unit counts as wounded / above which it is released.
    pub wounded_frac: f32,
    pub wounded_frac_release: f32,
    /// Cohesion leash radius (role.rs `LEASH`; perception's `LEASH_OUT`).
    pub leash: f32,
    /// Leash re-grab radius (inner band of the leash Schmitt trigger).
    pub leash_in: f32,
    /// Seconds between squad re-decisions.
    pub squad_think_interval: f32,
    // NOTE: `FOLLOW_DEADZONE` / `FLEE_DISTANCE` stay as consts in `squad_ai/perception.rs` — their only use
    // is inside the pure `resolve_goal` helper, kept config-free so it stays unit-testable without threading
    // this resource through it and its ~11 test call sites. So they are intentionally NOT fields here.
}

/// Squad-unit locomotion, ORCA collision-avoidance, pack cohesion, and action ranges (`squad.rs`,
/// `squad_ai/cohesion.rs`, `squad_ai/actions.rs`).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct SquadMoveTuning {
    /// Squad cruise speed (world units / s).
    pub unit_speed: f32,
    /// Minimum speed multiplier when maximally encumbered (a carried unit never stalls completely).
    pub min_encumber: f32,
    /// Heading slew rate.
    pub turn_speed: f32,
    pub orca_radius: f32,
    pub orca_time_horizon: f32,
    pub orca_query_radius: f32,
    /// Arrival radius: a unit within this of its goal is "there".
    pub arrive_radius: f32,
    /// Pack cohesion radius (neighbours within this pull together).
    pub pack_radius: f32,
    /// Blob radius (tight-formation clumping).
    pub blob_radius: f32,
    /// Progress epsilon for stuck detection.
    pub progress_eps: f32,
    /// Seconds of no-progress before a unit counts as pack-stuck.
    pub pack_stuck_time: f32,
    /// Cohesion anchor-tracking ease rate.
    pub anchor_ease: f32,
    /// Cooldown between a unit's spoken barks.
    pub utter_cooldown: f32,
    /// Researcher study range.
    pub study_range: f32,
    /// Medic heal range.
    pub heal_range: f32,
    /// Medic heal rate (HP / s).
    pub heal_rate: f32,
}

/// The watcher/boss locomotion + senses (`enemy.rs`). Its HP/zap-cadence/cull live in `sim.boss`; this is
/// how it *moves and looks*. Damage (`CONTACT_DPS`) and VFX durations stay in code.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct BossTuning {
    pub min_speed: f32,
    pub max_speed: f32,
    pub accel: f32,
    /// Heading-commit cosine (how sharply it may turn per step).
    pub turn_cos: f32,
    /// Near/far sight band.
    pub sight_near: f32,
    pub sight_far: f32,
    pub wander_interval: f32,
    pub sep_radius: f32,
    pub sep_strength: f32,
    /// How close the boss keeps while observing its quarry.
    pub observe_dist: f32,
    /// Head-look blend amount.
    pub look_amount: f32,
    /// Gaze cone cosine (how head-on a look must be to "see").
    pub gaze_cos: f32,
    /// Seconds the boss remembers a hit (drives its flee/retaliate).
    pub hit_memory: f32,
    pub flee_speed: f32,
}

/// Squad weapon ballistics + aim cone (`laser.rs`). Damage/friendly-fire live in `sim.combat`; this is
/// the fire cadence and spread model.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct LaserTuning {
    pub fire_interval: f32,
    pub laser_speed: f32,
    pub laser_life: f32,
    /// Base aim spread (radians-ish cone).
    pub base_spread: f32,
    /// Extra spread while the shooter is moving.
    pub move_spread: f32,
    /// Extra spread scaled by target distance.
    pub dist_spread: f32,
    /// Distance over which `dist_spread` ramps to full.
    pub dist_spread_range: f32,
    /// Front-arc cosine: a target outside this cone cannot be fired on.
    pub front_arc_cos: f32,
}

/// Crab-swarm locomotion, boids steering, pounce, scouts, caste promotion, and the carry crew (`crab.rs`).
/// Crab HP/damage/breeding live in `sim.combat`/`sim.breeding`; spawn counts/layout and render geometry
/// stay in code.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct CrabTuning {
    /// Contact range at which a crab's touch registers.
    pub contact_radius: f32,
    // ── pounce ──
    pub jump_len: f32,
    pub jump_min: f32,
    pub jump_hunker: f32,
    pub jump_air: f32,
    pub jump_arc: f32,
    pub jump_cooldown: f32,
    /// Cosine of the target's blind arc a crab must be inside to pounce.
    pub pounce_blind_cos: f32,
    pub stalk_band: f32,
    pub stalk_strength: f32,
    // ── locomotion / boids ──
    pub speed: f32,
    pub sep_radius: f32,
    pub sep_strength: f32,
    pub jitter_strength: f32,
    pub jitter_freq: f32,
    pub muster_speed_mul: f32,
    pub climb_speed: f32,
    pub flee_speed_mul: f32,
    // ── scouts ──
    pub scout_fraction: f32,
    pub scout_sight: f32,
    pub scout_speed_mul: f32,
    pub scout_wander_interval: f32,
    pub rally_deposit_cooldown: f32,
    // ── feeding ──
    pub eat_range: f32,
    /// Spawn spread of bred crabs behind the nest (shapes swarm distribution).
    pub back_spread: f32,
    // ── caste promotion ──
    pub caste_cooldown: f32,
    /// Max caste flips per tick (integer throttle; not searched).
    pub caste_flips_per_tick: usize,
    /// Rally-vector length above which a live beacon is nearby.
    pub rally_live: f32,
    /// Local ALARM above which fighters hold (defense press).
    pub alarm_high: f32,
    /// Local CRAB_DENSITY above which the area is crowded enough to spare a scout.
    pub promote_density: f32,
    pub scout_min_frac: f32,
    pub scout_max_frac: f32,
    // ── carry crew ──
    pub carry_capacity: f32,
    pub grab_range: f32,
    pub los_range: f32,
    pub carry_hold: f32,
    pub carry_speed: f32,
    pub weight_drag: f32,
    pub deliver_range: f32,
    pub crew_timeout: f32,
    pub max_commit_dist: f32,
}

/// SCP-150 manca collective behaviour: huddle/harborage, rouse contagion, boids swarm, commit ramp, flash,
/// and burst *choreography timing* (`parasite.rs`). Its count/hp/crawl/climb/leap-len/lethality live in
/// `sim.parasite`; render geometry, spawn jitter, wall-detection threshold, and the burst damage divisor
/// stay in code (the last to avoid double-searching parasite lethality with the world genome).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct ParasiteSwarmTuning {
    /// Range at which a manca can embed into a host.
    pub embed_range: f32,
    // ── leap / stalk shape ──
    pub leap_min: f32,
    pub leap_hunker: f32,
    pub leap_air: f32,
    pub leap_arc: f32,
    pub blind_cos: f32,
    pub stalk_band: f32,
    pub stalk_strength: f32,
    // ── huddle / harborage ──
    /// Huddle target group size (integer; not searched).
    pub huddle_size: usize,
    pub huddle_radius: f32,
    pub harborage_sep: f32,
    /// Minimum harborage quality score (integer; not searched).
    pub harborage_min_score: i32,
    pub settle_speed: f32,
    pub settle_arrive: f32,
    pub cohesion_strength: f32,
    pub huddle_sep_radius: f32,
    pub huddle_sep_strength: f32,
    pub harborage_bias: f32,
    // ── rouse / contagion ──
    pub rouse_threat: f32,
    pub rouse_proximity: f32,
    pub rouse_contagion_r: f32,
    pub rouse_calm_seconds: f32,
    // ── boids swarm ──
    pub align_strength: f32,
    pub align_floor: f32,
    pub seek_strength: f32,
    pub mill_speed_factor: f32,
    pub charge_speed_factor: f32,
    pub commit_ramp: f32,
    pub commit_decay: f32,
    pub commit_spread: f32,
    pub flash_secs: f32,
    pub flash_impulse: f32,
    pub flash_sep_boost: f32,
    // ── burst choreography timing ──
    pub burst_convulse_secs: f32,
    pub burst_bleed_secs: f32,
    pub bleed_interval: f32,
    pub emerge_secs: f32,
    pub emerge_dist: f32,
    pub embed_cooldown: f32,
    pub convulse_trauma_per_tick: f32,
    pub erupt_trauma: f32,
}

/// Crab↔mould ecosystem coupling: grazing/deposit rates and stigmergy splat radii (`mycelia/grazing.rs`,
/// `mycelia/control.rs`). The mould growth clock stays in `mycelia`; these are the *interaction* rates.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct MyceliaCouplingTuning {
    /// Cap on MEAT deposit / s from a grazed fruit body.
    pub fruit_meat_rate: f32,
    /// Bite reach of a grazing crab.
    pub graze_reach: f32,
    /// Growth eaten / crab / s while grazing.
    pub graze_bite_rate: f32,
    /// Biomass "smell" threshold at which a dense mat emits scent.
    pub mat_dense_v: f32,
    /// Mat scent deposited / s · biomass.
    pub mat_meat_rate: f32,
    pub nest_radius_cells: f32,
    pub unit_radius_cells: f32,
    pub blood_min_radius_cells: f32,
    pub meat_radius_cells: f32,
    /// Attention threshold above which the mould counts a cell as "seen" (habituation).
    pub gaze_seen: f32,
}

/// Root behaviour-tuning slice — the `behavior:` config section. Nested `Copy` sub-structs, one per
/// subsystem, so the RON reads as logical groups and the search can pick a subset per cluster. Extracted
/// from `GameConfig` and inserted as a standalone `Resource` by `AiPlugin::build` (the same extract-and-
/// insert pattern as `SimTuning`/`AudioTuning`), so systems read `Res<BehaviorTuning>` and the harness's
/// single `GameConfig` seam can install an evolved slice before that extraction runs.
#[derive(bevy::prelude::Resource, Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct BehaviorTuning {
    pub perception: PerceptionTuning,
    pub squad_move: SquadMoveTuning,
    pub boss: BossTuning,
    pub laser: LaserTuning,
    pub crab: CrabTuning,
    pub parasite_swarm: ParasiteSwarmTuning,
    pub mycelia_coupling: MyceliaCouplingTuning,
}

impl Default for BehaviorTuning {
    /// The shipped values — each equal to the const it replaced at migration time (a reviewable starting
    /// point; behaviour may be retuned freely since determinism, not byte-identity, is the invariant).
    fn default() -> Self {
        Self {
            perception: PerceptionTuning {
                examine_sight: 8.0,
                examine_sight_release: 9.0,
                threat_sight: 12.0,
                threat_sight_release: 13.5,
                psi_sight: 16.0,
                psi_sight_release: 17.5,
                ward_sight: 10.0,
                ward_sight_release: 11.5,
                wounded_frac: 0.5,
                wounded_frac_release: 0.55,
                leash: 6.0,
                leash_in: 4.0,
                squad_think_interval: 0.3,
            },
            squad_move: SquadMoveTuning {
                unit_speed: 6.0,
                min_encumber: 0.15,
                turn_speed: 14.0,
                orca_radius: 0.30,
                orca_time_horizon: 1.0,
                orca_query_radius: 4.0,
                arrive_radius: 0.6,
                pack_radius: 2.5,
                blob_radius: 1.3,
                progress_eps: 0.05,
                pack_stuck_time: 0.5,
                anchor_ease: 4.0,
                utter_cooldown: 4.0,
                study_range: 1.6,
                heal_range: 1.6,
                heal_rate: 20.0,
            },
            boss: BossTuning {
                min_speed: 0.4,
                max_speed: 2.5,
                accel: 0.4,
                turn_cos: 0.87,
                sight_near: 3.0,
                sight_far: 12.0,
                wander_interval: 2.5,
                sep_radius: 1.2,
                sep_strength: 2.5,
                observe_dist: 3.0,
                look_amount: 0.35,
                gaze_cos: 0.927,
                hit_memory: 0.6,
                flee_speed: 3.2,
            },
            laser: LaserTuning {
                fire_interval: 0.15,
                laser_speed: 22.0,
                laser_life: 1.2,
                base_spread: 0.06,
                move_spread: 0.40,
                dist_spread: 0.30,
                dist_spread_range: 14.0,
                front_arc_cos: 0.26,
            },
            crab: CrabTuning {
                contact_radius: 0.2,
                jump_len: 1.9,
                jump_min: 1.15,
                jump_hunker: 0.3,
                jump_air: 0.35,
                jump_arc: 0.9,
                jump_cooldown: 2.5,
                pounce_blind_cos: 0.5,
                stalk_band: 3.5,
                stalk_strength: 0.8,
                speed: 2.1,
                sep_radius: 0.45,
                sep_strength: 7.0,
                jitter_strength: 0.6,
                jitter_freq: 2.3,
                muster_speed_mul: 1.4,
                climb_speed: 2.89,
                flee_speed_mul: 1.3,
                scout_fraction: 0.20,
                scout_sight: 5.0,
                scout_speed_mul: 1.35,
                scout_wander_interval: 3.0,
                rally_deposit_cooldown: 0.2,
                eat_range: 0.3,
                back_spread: 2.6,
                caste_cooldown: 4.0,
                caste_flips_per_tick: 2,
                rally_live: 0.15,
                alarm_high: 0.5,
                promote_density: 3.0,
                scout_min_frac: 0.10,
                scout_max_frac: 0.30,
                carry_capacity: 0.4,
                grab_range: 0.6,
                los_range: 2.0,
                carry_hold: 0.12,
                carry_speed: 1.6,
                weight_drag: 2.5,
                deliver_range: 1.2,
                crew_timeout: 6.0,
                max_commit_dist: 2.5,
            },
            parasite_swarm: ParasiteSwarmTuning {
                embed_range: 0.2,
                leap_min: 1.0,
                leap_hunker: 0.3,
                leap_air: 0.35,
                leap_arc: 0.8,
                blind_cos: 0.5,
                stalk_band: 3.5,
                stalk_strength: 0.8,
                huddle_size: 4,
                huddle_radius: 2.0,
                harborage_sep: 6.0,
                harborage_min_score: 2,
                settle_speed: 0.5,
                settle_arrive: 0.6,
                cohesion_strength: 0.6,
                huddle_sep_radius: 0.35,
                huddle_sep_strength: 0.8,
                harborage_bias: 1.0,
                rouse_threat: 0.02,
                rouse_proximity: 7.0,
                rouse_contagion_r: 3.0,
                rouse_calm_seconds: 9.0,
                align_strength: 1.2,
                align_floor: 0.12,
                seek_strength: 0.5,
                mill_speed_factor: 0.5,
                charge_speed_factor: 1.35,
                commit_ramp: 0.5,
                commit_decay: 0.7,
                commit_spread: 2.0,
                flash_secs: 0.3,
                flash_impulse: 2.5,
                flash_sep_boost: 2.0,
                burst_convulse_secs: 1.5,
                burst_bleed_secs: 2.0,
                bleed_interval: 0.18,
                emerge_secs: 1.2,
                emerge_dist: 0.3,
                embed_cooldown: 6.0,
                convulse_trauma_per_tick: 0.006,
                erupt_trauma: 0.55,
            },
            mycelia_coupling: MyceliaCouplingTuning {
                fruit_meat_rate: 0.1,
                graze_reach: 0.35,
                graze_bite_rate: 0.15,
                mat_dense_v: 0.6,
                mat_meat_rate: 0.03,
                nest_radius_cells: 2.5,
                unit_radius_cells: 2.0,
                blood_min_radius_cells: 1.0,
                meat_radius_cells: 1.6,
                gaze_seen: 0.2,
            },
        }
    }
}

/// Range-check the behaviour knobs. One path, no fallback: an out-of-range value is a loud `Err` the
/// loader (`config::load_game_config`) surfaces, never a silent clamp. Every knob must be finite; speeds,
/// radii, cooldowns, and rates must be > 0; every Schmitt release band must be ≥ its trigger (else the
/// hysteresis inverts and the state chatters); the scout fraction band must be ordered.
pub fn validate_tuning(t: &BehaviorTuning) -> Result<(), String> {
    let finite = |name: &str, v: f32| -> Result<(), String> {
        if v.is_finite() {
            Ok(())
        } else {
            Err(format!("behavior.{name} must be finite (got {v})"))
        }
    };
    let positive = |name: &str, v: f32| -> Result<(), String> {
        if v.is_finite() && v > 0.0 {
            Ok(())
        } else {
            Err(format!("behavior.{name} must be finite and > 0 (got {v})"))
        }
    };
    let band = |name: &str, trigger: f32, release: f32| -> Result<(), String> {
        if release >= trigger {
            Ok(())
        } else {
            Err(format!(
                "behavior.{name}: release band ({release}) must be >= its trigger ({trigger}) — else the \
                 Schmitt hysteresis inverts and the state chatters"
            ))
        }
    };

    // Perception — sight radii positive; release ≥ trigger for every band.
    let p = &t.perception;
    positive("perception.examine_sight", p.examine_sight)?;
    positive("perception.threat_sight", p.threat_sight)?;
    positive("perception.psi_sight", p.psi_sight)?;
    positive("perception.ward_sight", p.ward_sight)?;
    positive("perception.wounded_frac", p.wounded_frac)?;
    positive("perception.leash", p.leash)?;
    positive("perception.leash_in", p.leash_in)?;
    positive("perception.squad_think_interval", p.squad_think_interval)?;
    band("perception.examine", p.examine_sight, p.examine_sight_release)?;
    band("perception.threat", p.threat_sight, p.threat_sight_release)?;
    band("perception.psi", p.psi_sight, p.psi_sight_release)?;
    band("perception.ward", p.ward_sight, p.ward_sight_release)?;
    band("perception.wounded", p.wounded_frac, p.wounded_frac_release)?;
    // The cohesion leash is a Schmitt trigger too: a unit strays past `leash` (the outer trigger) and is
    // released only inside `leash_in` (the inner re-grab). `latch_when_above` in `squad_ai::perception`
    // asserts the outer >= the inner, so an inverted band (e.g. an evolved `leash` mutated below the fixed
    // `leash_in`) is rejected here at the door, never a runtime panic.
    band("perception.leash", p.leash_in, p.leash)?;

    // Squad movement — speeds/radii/rates positive; a couple may be zero (deadzones/epsilons).
    let s = &t.squad_move;
    for (n, v) in [
        ("squad_move.unit_speed", s.unit_speed),
        ("squad_move.min_encumber", s.min_encumber),
        ("squad_move.turn_speed", s.turn_speed),
        ("squad_move.orca_radius", s.orca_radius),
        ("squad_move.orca_time_horizon", s.orca_time_horizon),
        ("squad_move.orca_query_radius", s.orca_query_radius),
        ("squad_move.arrive_radius", s.arrive_radius),
        ("squad_move.pack_radius", s.pack_radius),
        ("squad_move.blob_radius", s.blob_radius),
        ("squad_move.pack_stuck_time", s.pack_stuck_time),
        ("squad_move.anchor_ease", s.anchor_ease),
        ("squad_move.utter_cooldown", s.utter_cooldown),
        ("squad_move.study_range", s.study_range),
        ("squad_move.heal_range", s.heal_range),
        ("squad_move.heal_rate", s.heal_rate),
    ] {
        positive(n, v)?;
    }
    finite("squad_move.progress_eps", s.progress_eps)?;

    // Boss — the speed band must be ordered; senses positive; look/gaze cosines finite.
    let b = &t.boss;
    positive("boss.min_speed", b.min_speed)?;
    positive("boss.max_speed", b.max_speed)?;
    if b.max_speed < b.min_speed {
        return Err(format!(
            "behavior.boss.max_speed ({}) must be >= min_speed ({})",
            b.max_speed, b.min_speed
        ));
    }
    for (n, v) in [
        ("boss.accel", b.accel),
        ("boss.sight_near", b.sight_near),
        ("boss.sight_far", b.sight_far),
        ("boss.wander_interval", b.wander_interval),
        ("boss.sep_radius", b.sep_radius),
        ("boss.observe_dist", b.observe_dist),
        ("boss.hit_memory", b.hit_memory),
        ("boss.flee_speed", b.flee_speed),
    ] {
        positive(n, v)?;
    }
    for (n, v) in [
        ("boss.turn_cos", b.turn_cos),
        ("boss.sep_strength", b.sep_strength),
        ("boss.look_amount", b.look_amount),
        ("boss.gaze_cos", b.gaze_cos),
    ] {
        finite(n, v)?;
    }
    if b.sight_far < b.sight_near {
        return Err(format!(
            "behavior.boss.sight_far ({}) must be >= sight_near ({})",
            b.sight_far, b.sight_near
        ));
    }

    // Laser — cadence/speed/life positive; spreads finite and non-negative.
    let l = &t.laser;
    for (n, v) in [
        ("laser.fire_interval", l.fire_interval),
        ("laser.laser_speed", l.laser_speed),
        ("laser.laser_life", l.laser_life),
        ("laser.dist_spread_range", l.dist_spread_range),
    ] {
        positive(n, v)?;
    }
    for (n, v) in [
        ("laser.base_spread", l.base_spread),
        ("laser.move_spread", l.move_spread),
        ("laser.dist_spread", l.dist_spread),
    ] {
        if !(v.is_finite() && v >= 0.0) {
            return Err(format!("behavior.{n} must be finite and >= 0 (got {v})"));
        }
    }
    finite("laser.front_arc_cos", l.front_arc_cos)?;

    // Crab — speeds/ranges/cooldowns positive; the scout fraction band ordered; cosines/thresholds finite.
    let c = &t.crab;
    for (n, v) in [
        ("crab.contact_radius", c.contact_radius),
        ("crab.jump_len", c.jump_len),
        ("crab.jump_cooldown", c.jump_cooldown),
        ("crab.stalk_band", c.stalk_band),
        ("crab.speed", c.speed),
        ("crab.sep_radius", c.sep_radius),
        ("crab.muster_speed_mul", c.muster_speed_mul),
        ("crab.climb_speed", c.climb_speed),
        ("crab.flee_speed_mul", c.flee_speed_mul),
        ("crab.scout_sight", c.scout_sight),
        ("crab.scout_speed_mul", c.scout_speed_mul),
        ("crab.scout_wander_interval", c.scout_wander_interval),
        ("crab.rally_deposit_cooldown", c.rally_deposit_cooldown),
        ("crab.eat_range", c.eat_range),
        ("crab.caste_cooldown", c.caste_cooldown),
        ("crab.grab_range", c.grab_range),
        ("crab.los_range", c.los_range),
        ("crab.carry_speed", c.carry_speed),
        ("crab.deliver_range", c.deliver_range),
        ("crab.crew_timeout", c.crew_timeout),
        ("crab.max_commit_dist", c.max_commit_dist),
    ] {
        positive(n, v)?;
    }
    for (n, v) in [
        ("crab.jump_min", c.jump_min),
        ("crab.jump_hunker", c.jump_hunker),
        ("crab.jump_air", c.jump_air),
        ("crab.jump_arc", c.jump_arc),
        ("crab.pounce_blind_cos", c.pounce_blind_cos),
        ("crab.stalk_strength", c.stalk_strength),
        ("crab.sep_strength", c.sep_strength),
        ("crab.jitter_strength", c.jitter_strength),
        ("crab.jitter_freq", c.jitter_freq),
        ("crab.scout_fraction", c.scout_fraction),
        ("crab.back_spread", c.back_spread),
        ("crab.rally_live", c.rally_live),
        ("crab.alarm_high", c.alarm_high),
        ("crab.promote_density", c.promote_density),
        ("crab.carry_capacity", c.carry_capacity),
        ("crab.carry_hold", c.carry_hold),
        ("crab.weight_drag", c.weight_drag),
    ] {
        finite(n, v)?;
    }
    if c.scout_max_frac < c.scout_min_frac {
        return Err(format!(
            "behavior.crab.scout_max_frac ({}) must be >= scout_min_frac ({})",
            c.scout_max_frac, c.scout_min_frac
        ));
    }

    // Parasite swarm — speeds/ranges/cooldowns positive; steering weights and burst timing finite.
    let ps = &t.parasite_swarm;
    for (n, v) in [
        ("parasite_swarm.embed_range", ps.embed_range),
        ("parasite_swarm.leap_min", ps.leap_min),
        ("parasite_swarm.stalk_band", ps.stalk_band),
        ("parasite_swarm.huddle_radius", ps.huddle_radius),
        ("parasite_swarm.harborage_sep", ps.harborage_sep),
        ("parasite_swarm.settle_speed", ps.settle_speed),
        ("parasite_swarm.settle_arrive", ps.settle_arrive),
        ("parasite_swarm.huddle_sep_radius", ps.huddle_sep_radius),
        ("parasite_swarm.rouse_proximity", ps.rouse_proximity),
        ("parasite_swarm.rouse_contagion_r", ps.rouse_contagion_r),
        ("parasite_swarm.rouse_calm_seconds", ps.rouse_calm_seconds),
        ("parasite_swarm.charge_speed_factor", ps.charge_speed_factor),
        ("parasite_swarm.burst_convulse_secs", ps.burst_convulse_secs),
        ("parasite_swarm.burst_bleed_secs", ps.burst_bleed_secs),
        ("parasite_swarm.bleed_interval", ps.bleed_interval),
        ("parasite_swarm.emerge_secs", ps.emerge_secs),
        ("parasite_swarm.embed_cooldown", ps.embed_cooldown),
    ] {
        positive(n, v)?;
    }
    for (n, v) in [
        ("parasite_swarm.leap_hunker", ps.leap_hunker),
        ("parasite_swarm.leap_air", ps.leap_air),
        ("parasite_swarm.leap_arc", ps.leap_arc),
        ("parasite_swarm.blind_cos", ps.blind_cos),
        ("parasite_swarm.stalk_strength", ps.stalk_strength),
        ("parasite_swarm.cohesion_strength", ps.cohesion_strength),
        ("parasite_swarm.huddle_sep_strength", ps.huddle_sep_strength),
        ("parasite_swarm.harborage_bias", ps.harborage_bias),
        ("parasite_swarm.rouse_threat", ps.rouse_threat),
        ("parasite_swarm.align_strength", ps.align_strength),
        ("parasite_swarm.align_floor", ps.align_floor),
        ("parasite_swarm.seek_strength", ps.seek_strength),
        ("parasite_swarm.mill_speed_factor", ps.mill_speed_factor),
        ("parasite_swarm.commit_ramp", ps.commit_ramp),
        ("parasite_swarm.commit_decay", ps.commit_decay),
        ("parasite_swarm.commit_spread", ps.commit_spread),
        ("parasite_swarm.flash_secs", ps.flash_secs),
        ("parasite_swarm.flash_impulse", ps.flash_impulse),
        ("parasite_swarm.flash_sep_boost", ps.flash_sep_boost),
        ("parasite_swarm.emerge_dist", ps.emerge_dist),
        ("parasite_swarm.convulse_trauma_per_tick", ps.convulse_trauma_per_tick),
        ("parasite_swarm.erupt_trauma", ps.erupt_trauma),
    ] {
        finite(n, v)?;
    }
    if ps.huddle_size == 0 {
        return Err("behavior.parasite_swarm.huddle_size must be > 0".to_string());
    }

    // Mycelia coupling — rates and radii positive.
    let m = &t.mycelia_coupling;
    for (n, v) in [
        ("mycelia_coupling.fruit_meat_rate", m.fruit_meat_rate),
        ("mycelia_coupling.graze_reach", m.graze_reach),
        ("mycelia_coupling.graze_bite_rate", m.graze_bite_rate),
        ("mycelia_coupling.mat_dense_v", m.mat_dense_v),
        ("mycelia_coupling.mat_meat_rate", m.mat_meat_rate),
        ("mycelia_coupling.nest_radius_cells", m.nest_radius_cells),
        ("mycelia_coupling.unit_radius_cells", m.unit_radius_cells),
        ("mycelia_coupling.blood_min_radius_cells", m.blood_min_radius_cells),
        ("mycelia_coupling.meat_radius_cells", m.meat_radius_cells),
        ("mycelia_coupling.gaze_seen", m.gaze_seen),
    ] {
        positive(n, v)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_valid() {
        validate_tuning(&BehaviorTuning::default())
            .expect("shipped default behaviour tuning must validate");
    }

    #[test]
    fn behavior_default_equals_shipped_config() {
        // Byte-identity guard for the const→config migration (mirrors `audio_default_equals_shipped_config`):
        // the `behavior:` slice in the shipped `config.ron` must equal `BehaviorTuning::default()` exactly,
        // so a transcription typo reds this test instead of silently shifting a knob. No `#[serde(default)]`
        // on the slice — a missing/renamed field is a loud parse error, not a silent fallback.
        let cfg = crate::config::load_game_config().expect("shipped game config must load");
        assert_eq!(
            cfg.behavior,
            BehaviorTuning::default(),
            "assets/config/config.ron `behavior:` slice drifted from the shipped BehaviorTuning defaults"
        );
    }

    #[test]
    fn rejects_inverted_schmitt_band() {
        let mut t = BehaviorTuning::default();
        t.perception.threat_sight_release = t.perception.threat_sight - 1.0; // release below trigger
        assert!(validate_tuning(&t).is_err());
    }

    #[test]
    fn rejects_nonpositive_speed() {
        let mut t = BehaviorTuning::default();
        t.squad_move.unit_speed = 0.0;
        assert!(validate_tuning(&t).is_err());
    }

    #[test]
    fn rejects_disordered_scout_fraction_band() {
        let mut t = BehaviorTuning::default();
        t.crab.scout_max_frac = t.crab.scout_min_frac - 0.01;
        assert!(validate_tuning(&t).is_err());
    }
}
