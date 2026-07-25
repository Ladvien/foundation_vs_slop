//! The SCP-1048 behaviour executor — turns the mode the utility brain chose into movement and a pose.
//!
//! Runs on `FixedUpdate` `.after(AiSet::Think)`, so it reads *this* tick's `ActiveBehavior`. Movement
//! reuses `Dungeon::resolve_move` wall-sliding, the same solver the squad and SCP-999 use; the bears
//! deliberately do not ride the surface manifold (they are floor creatures, so the module needs no
//! `SurfaceGraph` and can seed on plain `Startup`).
//!
//! **This file owns motion and pose only.** What an attack *does* — damage, dread, ear growths — lives
//! in [`super::effects`], so the decision-to-pose mapping stays readable and the consequences sit next
//! to the determinism notes they need.
//!
//! The one `match variant` in the whole design is here, in [`strike_pose`]: A screams, B throws a
//! looping tantrum, C runs an aim → fire → whip gun chain. Everything else is variant-agnostic.

use bevy::prelude::*;

use super::{AnimState, Scp1048, Scp1048Seed, Scp1048State, Scp1048Variant, MAX_FRAME_DT, SCP1048_HALF};
use crate::ai::brain::ActiveBehavior;
use crate::ai::utility::Mode;
use crate::dungeon::Dungeon;
use crate::sim::SimTuning;
use crate::util::hash01_u32;

/// Authored clip lengths (seconds), read from the shipped glbs. The **state machine** owns how long a
/// timed pose is held — never `PoseBlender::active_shot()` — so these mirror the assets and are pinned
/// by `tests/creature_clip_contract.rs`'s duration checks.
mod clip_secs {
    /// `sit_down`: stands → folds down onto its bottom. Its last frame is `draw_picture`'s first.
    pub(super) const SIT_DOWN: f32 = 2.042;
    /// `jump_in_place`: a hop that actually leaves the ground.
    pub(super) const JUMP: f32 = 1.708;
    /// `scream` (SCP-1048-A): head thrown back, held on a sustained shiver.
    pub(super) const SCREAM: f32 = 1.375;
    /// `aim_gun` (SCP-1048-C): raises the arm cannon to a level aim, then holds its last frame.
    pub(super) const AIM: f32 = 0.542;
    /// `pistol_whip` (SCP-1048-C): the overhead club-down.
    pub(super) const WHIP: f32 = 0.708;
}

/// How the original picks its emote. Stable per bear (drawn from its immortal spawn seed), so each one
/// has a characteristic display rather than re-rolling every tick — which would read as a twitch and,
/// worse, would make the pose depend on tick count rather than identity.
fn emote_style(seed: u32) -> u32 {
    (hash01_u32(seed ^ 0x1048_BEA5) * 3.0) as u32 % 3
}

/// The pose an attacking copy should be in — **the** variant branch.
///
/// Advances `state` in place because C's gun chain is a two-phase sub-state (raise once, then fire per
/// cooldown) rather than a single clip.
fn strike_pose(variant: Scp1048Variant, state: &mut Scp1048State, dist: f32, t: &SimTuning) {
    match variant {
        // Should not happen — the benign brain has no `Mode::Strike`. Hold the idle rather than
        // inventing an attack for a bear that has no attack clip.
        Scp1048Variant::Original => state.anim = AnimState::RestIdle,
        // A: a discrete shriek on the cooldown, otherwise loom. `scream` returns to neutral, so it
        // cross-fades cleanly from the rage idle either way.
        Scp1048Variant::EarCopy => {
            if state.strike_cd <= 0.0 {
                state.anim = AnimState::Attack;
                state.phase_timer = clip_secs::SCREAM;
                state.strike_cd = t.scp1048.strike_cooldown;
                state.strike_landed = true;
            } else if state.phase_timer <= 0.0 {
                state.anim = AnimState::Rage;
            }
        }
        // B: the tantrum LOOPS, so it is held as a state for as long as the brain stays in Strike.
        // There is no clip edge to trigger — but the blows still land on a cadence, which is what the
        // cooldown carries here.
        Scp1048Variant::InfantArm => {
            state.anim = AnimState::Attack;
            if state.strike_cd <= 0.0 {
                state.strike_cd = t.scp1048.strike_cooldown;
                state.strike_landed = true;
            }
        }
        // C: raise the cannon once (holding its last frame — the aim pose), then replay `fire_gun` per
        // cooldown. Inside half the strike range it clubs instead, which gives the gun bear a distinct
        // close-quarters answer rather than firing point-blank.
        Scp1048Variant::Scrap => {
            if dist <= t.scp1048.strike_range * 0.5 {
                if state.strike_cd <= 0.0 {
                    state.anim = AnimState::Whip;
                    state.phase_timer = clip_secs::WHIP;
                    state.strike_cd = t.scp1048.strike_cooldown;
                    state.strike_landed = true;
                }
            } else if !state.aimed {
                state.anim = AnimState::Aim;
                state.phase_timer = clip_secs::AIM;
                state.aimed = true;
            } else if state.phase_timer <= 0.0 && state.strike_cd <= 0.0 {
                // `fire_gun` starts and ends in the aim pose, so staying on this slot afterwards leaves
                // the bear holding its aim — there is never a fade back to `aim_gun`.
                state.anim = AnimState::Fire;
                state.strike_cd = t.scp1048.strike_cooldown;
                state.strike_landed = true;
            }
        }
    }
}

/// Mode → movement + pose, once per bear per fixed tick.
///
/// ## Determinism
/// Every write here is **per-entity**: each bear reads its own `ActiveBehavior` and writes its own
/// `Transform` and `Scp1048State`. Nothing is accumulated into a shared field, no shared RNG is
/// advanced, and no budget is claimed first-come — so ECS iteration order cannot change the result and
/// no total sort is required. (The two places in this module that *do* decide from shared state — the
/// field deposits in `effects` and the replication spawner — carry their own sorts.)
pub(crate) fn scp1048_act(
    time: Res<Time>,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    mut bears: Query<(
        &Scp1048,
        &Scp1048Seed,
        &ActiveBehavior,
        &mut Scp1048State,
        &mut Transform,
    )>,
) {
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    if dt <= 0.0 {
        return;
    }
    let t = &sim.scp1048;

    for (bear, seed, active, mut state, mut tf) in &mut bears {
        // Timers run down regardless of mode, so a bear that is interrupted mid-pose still recovers.
        state.phase_timer = (state.phase_timer - dt).max(0.0);
        state.strike_cd = (state.strike_cd - dt).max(0.0);
        // Clear last tick's blow HERE rather than in `effects`. Both systems run on `FixedUpdate` in a
        // pinned order (act → effects), so clearing at the top of act means effects always reads the
        // flag this tick set — and effects never needs mutable access to bear state at all.
        state.strike_landed = false;

        let pos = tf.translation;
        let dist = active.target.map_or(f32::MAX, |g| (g.xz() - pos.xz()).length());

        // A timed pose owns the bear until it expires — except for the seated chain, which advances
        // into `Draw` the instant `sit_down` completes (their frames match, so it is seamless).
        if state.phase_timer > 0.0 && state.anim == AnimState::SitDown {
            continue;
        }
        if state.anim == AnimState::SitDown && state.phase_timer <= 0.0 {
            state.anim = AnimState::Draw;
            continue;
        }

        // Anything but the gun chain drops the raised-cannon latch, so C re-aims next engagement.
        if !matches!(active.mode, Mode::Strike) {
            state.aimed = false;
        }

        let mut goal: Option<Vec3> = None;
        match active.mode {
            // Drift. `ActiveBehavior` carries no wander target for these brains, so the bear simply
            // holds — the level is small and a bear that stands and breathes reads as watchful.
            Mode::Wander => state.anim = AnimState::RestIdle,
            // The endearing display. Each bear has a characteristic one, keyed off its spawn seed.
            Mode::Emote => {
                state.anim = match emote_style(seed.0) {
                    0 => AnimState::Dance,
                    1 => {
                        if state.anim != AnimState::Draw {
                            state.phase_timer = clip_secs::SIT_DOWN;
                            AnimState::SitDown
                        } else {
                            AnimState::Draw
                        }
                    }
                    _ => {
                        if state.anim != AnimState::Jump {
                            state.phase_timer = clip_secs::JUMP;
                        }
                        AnimState::Jump
                    }
                };
            }
            // Scavenging is a still, unobtrusive beat — the material accrual itself lives in
            // `super::replicate`. Standing quietly is the point: it is what the squad fails to notice.
            Mode::Build => state.anim = AnimState::RestIdle,
            // Bolt away from whatever the bear is afraid of. With no known threat it simply retreats
            // from its current target.
            Mode::Flee => {
                state.anim = AnimState::RestIdle;
                if let Some(threat) = active.target {
                    let away = (pos.xz() - threat.xz()).normalize_or_zero();
                    if away != Vec2::ZERO {
                        goal = Some(pos + Vec3::new(away.x, 0.0, away.y));
                    }
                }
            }
            // The threat display: loom in place. Closing is `Chase`'s job.
            Mode::Rage => state.anim = AnimState::Rage,
            // Close to contact, still looming.
            Mode::Chase => {
                state.anim = AnimState::Rage;
                goal = active.target;
            }
            Mode::Strike => strike_pose(bear.variant, &mut state, dist, &sim),
            // Every other mode belongs to a squad role or another creature and can never be chosen by
            // a bear brain. Enumerated rather than caught by `_` so that adding a bear mode without
            // giving it an executor arm is a COMPILE ERROR, not a bear that silently stands still.
            Mode::Forage
            | Mode::Latch
            | Mode::HuntBlood
            | Mode::SeekMeat
            | Mode::Carry
            | Mode::Scout
            | Mode::Mark
            | Mode::Rally
            | Mode::Muster
            | Mode::Investigate
            | Mode::Examine
            | Mode::Overwatch
            | Mode::Engage
            | Mode::Suppress
            | Mode::PsiScan
            | Mode::Commune
            | Mode::Ward
            | Mode::TendWounded
            | Mode::SecureDoor
            | Mode::DeploySensor
            | Mode::Regroup
            | Mode::FollowAnchor => state.anim = AnimState::RestIdle,
        }

        // Stop short of the target so a chasing copy settles into strike range instead of grinding
        // into the unit it is attacking.
        if let Some(g) = goal {
            let to = g.xz() - pos.xz();
            if to.length() > t.strike_range {
                let dir = to.normalize_or_zero();
                if dir != Vec2::ZERO {
                    let step = Vec3::new(dir.x, 0.0, dir.y) * (t.move_speed * dt);
                    let resolved = dungeon.resolve_move(pos, step, SCP1048_HALF);
                    tf.translation.x = resolved.x;
                    tf.translation.z = resolved.z;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> SimTuning {
        SimTuning::default()
    }

    #[test]
    fn emote_style_is_stable_per_seed_and_spans_every_display() {
        for seed in 0u32..64 {
            assert_eq!(emote_style(seed), emote_style(seed), "an emote must not re-roll");
            assert!(emote_style(seed) < 3);
        }
        let styles: std::collections::HashSet<u32> = (0u32..4000).map(emote_style).collect();
        assert_eq!(styles.len(), 3, "all three displays should be reachable across seeds");
    }

    #[test]
    fn the_ear_bear_screams_on_the_cooldown_and_looms_between() {
        let t = tuning();
        let mut s = Scp1048State::new();
        strike_pose(Scp1048Variant::EarCopy, &mut s, 1.0, &t);
        assert_eq!(s.anim, AnimState::Attack, "a ready ear bear screams");
        assert!(s.strike_cd > 0.0, "screaming must start the cooldown");
        // Mid-cooldown, with the scream clip finished, it falls back to the threat display.
        s.phase_timer = 0.0;
        strike_pose(Scp1048Variant::EarCopy, &mut s, 1.0, &t);
        assert_eq!(s.anim, AnimState::Rage);
    }

    #[test]
    fn the_infant_arm_bear_holds_its_tantrum_as_a_state() {
        // The looping attack must not depend on a cooldown edge — it is held for as long as the brain
        // stays in Strike, so re-entering must leave it in Attack rather than cycling to Rage.
        let t = tuning();
        let mut s = Scp1048State::new();
        for _ in 0..5 {
            strike_pose(Scp1048Variant::InfantArm, &mut s, 1.0, &t);
            assert_eq!(s.anim, AnimState::Attack);
        }
    }

    #[test]
    fn the_scrap_bear_raises_its_gun_once_then_fires() {
        let t = tuning();
        let mut s = Scp1048State::new();
        // Well outside whip range: first the aim.
        strike_pose(Scp1048Variant::Scrap, &mut s, 1.0, &t);
        assert_eq!(s.anim, AnimState::Aim);
        assert!(s.aimed, "the raise latches so it is not replayed every tick");
        // Aim clip done, cooldown clear ⇒ a shot.
        s.phase_timer = 0.0;
        s.strike_cd = 0.0;
        strike_pose(Scp1048Variant::Scrap, &mut s, 1.0, &t);
        assert_eq!(s.anim, AnimState::Fire);
        assert!(s.strike_cd > 0.0);
    }

    #[test]
    fn the_scrap_bear_clubs_at_point_blank_instead_of_firing() {
        let t = tuning();
        let mut s = Scp1048State::new();
        strike_pose(Scp1048Variant::Scrap, &mut s, t.scp1048.strike_range * 0.25, &t);
        assert_eq!(s.anim, AnimState::Whip);
    }

    #[test]
    fn the_benign_original_has_no_attack_pose() {
        // Its brain carries no `Mode::Strike`, but if that ever changed this must not invent an attack
        // for a bear whose glb ships no attack clip.
        let t = tuning();
        let mut s = Scp1048State::new();
        strike_pose(Scp1048Variant::Original, &mut s, 1.0, &t);
        assert_eq!(s.anim, AnimState::RestIdle);
    }
}
