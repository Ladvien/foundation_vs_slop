//! TEMPORARY numeric diagnostics for the AI layer — the game window can't be reliably screenshotted
//! (it occludes and returns black frames), so emergence is verified by logging objective numbers.
//! Gated behind [`AI_DIAG`]; flip to `false` (or delete the systems) once a behaviour is confirmed.

use bevy::prelude::*;

use super::brain::ActiveBehavior;
use super::drives::{DriveId, Drives};
use super::field::{FieldId, Stig};
use super::utility::Mode;
use crate::crab::Crab;
use crate::dungeon::Dungeon;
use crate::enemy::Enemy;

/// Master switch for the diagnostic logging systems. Flip to `true` to watch fields/drives/modes in
/// the console when tuning emergence; `false` for normal play (systems early-return, ~free).
pub const AI_DIAG: bool = false;

/// Log each channel's peak value + location once per second (verifies deposit/evaporate/diffuse).
pub fn log_fields(
    time: Res<Time>,
    mut t: Local<f32>,
    stig: Option<Res<Stig>>,
    dungeon: Option<Res<Dungeon>>,
) {
    if !AI_DIAG {
        return;
    }
    let (Some(stig), Some(dungeon)) = (stig, dungeon) else {
        return;
    };
    *t += time.delta_secs();
    if *t < 1.0 {
        return;
    }
    *t = 0.0;
    let (sc_pos, sc) = stig.hotspot(FieldId::SCENT, &dungeon);
    let (th_pos, th) = stig.hotspot(FieldId::THREAT, &dungeon);
    let (cd_pos, cd) = stig.hotspot(FieldId::CRAB_DENSITY, &dungeon);
    let (mt_pos, mt) = stig.hotspot(FieldId::MEAT, &dungeon);
    let (rl_pos, rl) = stig.hotspot(FieldId::RALLY, &dungeon);
    info!(
        "ai-fields: scent={sc:.2}@{:?} threat={th:.2}@{:?} density={cd:.2}@{:?} meat={mt:.2}@{:?} rally={rl:.2}@{:?}",
        sc_pos.xz(),
        th_pos.xz(),
        cd_pos.xz(),
        mt_pos.xz(),
        rl_pos.xz()
    );
}

/// Log crab drive distribution once per second (verifies drives respond to the fields).
pub fn log_drives(time: Res<Time>, mut t: Local<f32>, crabs: Query<&Drives, With<Crab>>) {
    if !AI_DIAG {
        return;
    }
    *t += time.delta_secs();
    if *t < 1.0 {
        return;
    }
    *t = 0.0;
    let mut n = 0.0f32;
    let mut hunger = 0.0f32;
    let mut fear_sum = 0.0f32;
    let mut fear_max = 0.0f32;
    for d in &crabs {
        n += 1.0;
        hunger += d.get(DriveId::HUNGER);
        let f = d.get(DriveId::FEAR);
        fear_sum += f;
        fear_max = fear_max.max(f);
    }
    if n > 0.0 {
        info!(
            "ai-drives: crabs={n} hunger_mean={:.2} fear_mean={:.2} fear_max={:.2}",
            hunger / n,
            fear_sum / n,
            fear_max
        );
    }
}

/// Log the boss's chosen mode + how far it is from the scent hotspot (verifies drawn-to-blood: when
/// it picks `HuntBlood`, that distance should shrink over time).
pub fn log_boss(
    time: Res<Time>,
    mut t: Local<f32>,
    stig: Option<Res<Stig>>,
    dungeon: Option<Res<Dungeon>>,
    boss: Query<(&Transform, &ActiveBehavior), With<Enemy>>,
) {
    if !AI_DIAG {
        return;
    }
    let (Some(stig), Some(dungeon)) = (stig, dungeon) else {
        return;
    };
    *t += time.delta_secs();
    if *t < 1.0 {
        return;
    }
    *t = 0.0;
    let (scent_pos, scent_val) = stig.hotspot(FieldId::SCENT, &dungeon);
    for (tf, active) in &boss {
        let d = (tf.translation.xz() - scent_pos.xz()).length();
        info!(
            "ai-boss: mode={:?} scent_hotspot={scent_val:.2} dist_to_hotspot={d:.1}",
            active.mode
        );
    }
}

/// Log the crab mode histogram + positional spread (std-dev) — the headline frenzy→scatter signal:
/// gunfire → THREAT → FEAR → the `Flee` count spikes and the spread widens; then it re-forms.
pub fn log_crab_modes(
    time: Res<Time>,
    mut t: Local<f32>,
    crabs: Query<(&ActiveBehavior, &Transform), With<Crab>>,
) {
    if !AI_DIAG {
        return;
    }
    *t += time.delta_secs();
    if *t < 1.0 {
        return;
    }
    *t = 0.0;
    let mut forage = 0;
    let mut latch = 0;
    let mut flee = 0;
    let mut seek = 0;
    let mut carry = 0;
    let mut scout = 0;
    let mut report = 0;
    let mut rally = 0;
    let mut other = 0;
    let mut mean = Vec2::ZERO;
    let mut n = 0.0f32;
    for (active, tf) in &crabs {
        match active.mode {
            Mode::Forage => forage += 1,
            Mode::Latch => latch += 1,
            Mode::Flee => flee += 1,
            Mode::SeekMeat => seek += 1,
            Mode::Carry => carry += 1,
            Mode::Scout => scout += 1,
            Mode::Report => report += 1,
            Mode::Rally => rally += 1,
            _ => other += 1,
        }
        mean += tf.translation.xz();
        n += 1.0;
    }
    if n == 0.0 {
        return;
    }
    mean /= n;
    let mut var = 0.0f32;
    for (_, tf) in &crabs {
        var += (tf.translation.xz() - mean).length_squared();
    }
    let spread = (var / n).sqrt();
    info!(
        "ai-crabs: forage={forage} latch={latch} flee={flee} seek={seek} carry={carry} scout={scout} report={report} rally={rally} other={other} spread={spread:.1}"
    );
}

/// Log per-gib crew status (Phase 3/4): how many chunks exist, how many have a full crew (Σ capacity ≥
/// weight), and the mean crab→nearest-gib distance for foraging crabs (should shrink as they converge).
pub fn log_crew(
    time: Res<Time>,
    mut t: Local<f32>,
    crabs: Query<(&Transform, &crate::crab::CrabCarry), With<Crab>>,
    gibs: Query<(&Transform, &crate::gore::Carryable)>,
    nests: Query<&crate::nest::Nest>,
) {
    if !AI_DIAG {
        return;
    }
    *t += time.delta_secs();
    if *t < 1.0 {
        return;
    }
    *t = 0.0;

    let mut gib_n = 0;
    let mut crewing = 0;
    let mut hauling = 0;
    for (_, carry) in &gibs {
        gib_n += 1;
        match carry.phase {
            crate::gore::CarryPhase::Crewing => crewing += 1,
            crate::gore::CarryPhase::Hauling => hauling += 1,
            crate::gore::CarryPhase::Resting => {}
        }
    }
    let hoard: f32 = nests.iter().map(|n| n.hoard).sum();

    // Mean distance from each SeekMeat-ish crab (has a target) to its target gib.
    let mut dsum = 0.0f32;
    let mut dn = 0.0f32;
    for (tf, cc) in &crabs {
        if let Some(g) = cc.target {
            if let Ok((gtf, _)) = gibs.get(g) {
                dsum += tf.translation.distance(gtf.translation);
                dn += 1.0;
            }
        }
    }
    let mean_d = if dn > 0.0 { dsum / dn } else { -1.0 };
    info!(
        "ai-crew: gibs={gib_n} crewing={crewing} hauling={hauling} committed_crabs={dn} mean_crab_to_gib={mean_d:.2} hoard={hoard:.1}"
    );
}
