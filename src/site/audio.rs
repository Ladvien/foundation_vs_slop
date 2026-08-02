//! **The hub's own sound** — footfalls that exist, and a room tone per wing.
//!
//! # What was actually missing
//!
//! The handoff note said the Site was "completely silent". It is not, and the difference matters:
//! `audio::load_audio` spawns the wind bed and the calm music loop at `Startup` with no state gate,
//! and `audio.rs:1023` records that being a deliberate fix — *"audio must still RUN there (music and
//! the ambient bed are not expedition state)"*. Booting straight to Site-67 you hear both.
//!
//! What the hub had none of was anything **it** was making:
//!
//! * **No footfalls.** `audio::footsteps` queries `(&Velocity, &Transform), With<Unit>`. Site avatars
//!   are `SiteAvatar` by deliberate design (`site::mod` gives the reason in full), they carry no
//!   `Velocity` at all, and they are moved by writing `Transform` directly. Zero movers, early
//!   return — so seven staff and five operatives walked a concrete facility in perfect silence.
//! * **No one-shots.** `audio::ambient_oneshots` anchors on the `Unit` centroid and returns when
//!   there is none, which at the Site is always.
//! * **No per-room identity.** Which is the thing worth having, and the thing this module is for.
//!
//! # Room tone as the acoustic half of the lighting
//!
//! `visuals::area_light` gives each wing its own colour temperature, and the lighting note's claim is
//! that *"you can tell which wing you are in from the colour of the air"*. [`area_tone`] is that
//! sentence for sound, and it is deliberately the same shape — a pure `match` on [`AreaId`] returning
//! a small spec — so the two read as one decision made twice rather than two systems that happen to
//! agree.
//!
//! It uses the palette already shipped (`audio/ambience/oneshot/`: six creaks, two drips, a clock)
//! rather than importing new beds. That is a real constraint honestly taken: nine clips cannot give
//! twelve rooms twelve *timbres*, but they can give each wing a different **rhythm and register** —
//! the archive ticks, the containment wing drips, the living half creaks — and the cadence is as
//! legible as the sample. Looping per-wing beds from the library are the obvious next step and are
//! not in this pass.
//!
//! # Cosmetic by construction
//!
//! `Update`, windowed-only, gated on `AppState::Site`. It spawns `AudioPlayer` entities with a
//! `Transform` and no `Health`, so nothing here can reach `snapshot_hash`. The randomness is a
//! `Local<u32>` LCG, never `DetRng` — this must not consume from a seeded stream that the simulation
//! also draws on.

use bevy::prelude::*;

use super::layout::AreaId;
use super::presence::CurrentArea;
use super::visuals::{AvatarGoal, PlayerAvatar, SiteAvatar};
use crate::audio::{jitter, one_shot_spatial, pick_variant, AudioAssets, AudioBus};
use crate::ui::state::AppState;

/// Seconds between footfalls for one walker. Shorter than the squad's `STRIDE` because a hub avatar
/// walks rather than advances under fire, and because there is no cover to be quiet behind.
const SITE_STRIDE: f32 = 0.46;
/// Floor on the shared voice's interval, for the same reason `audio::MIN_STRIDE` exists: reproducing
/// the true event rate through one voice is what made a five-person squad read as an army.
const SITE_MIN_STRIDE: f32 = 0.14;
/// How far an avatar must still be from its goal to count as walking. `drive_avatars` eases into the
/// goal, so a pure "has it moved" test would tick forever as the position converged.
const WALKING_EPSILON: f32 = 0.12;
/// Quieter than the expedition's boots: this is floor texture in a room you are safe in.
const SITE_FOOT_VOL: f32 = 0.30;
/// Room tone gain. Below the wind bed (0.22) on purpose — the acoustic program's §5 rule is that the
/// informative layer wins, and in the hub that is the staff and the interface.
const TONE_VOL: f32 = 0.26;

/// A wing's acoustic signature: which slice of the one-shot palette it draws from, and how often.
///
/// Mirrors `visuals::AreaLight` deliberately, down to being a plain `Copy` struct returned from a
/// total `match`. Two systems, one decision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaTone {
    /// Half-open index range into `AudioAssets::ambient_oneshots` — `[0,6)` are creaks, `[6,8)` the
    /// drips, `[8,9)` the clock.
    pub clips: (usize, usize),
    /// Shortest gap between events, seconds.
    pub min_gap: f32,
    /// Longest gap. A **range**, not a period: a regular tick becomes a tell within a minute, and an
    /// irregular one stays a low-grade expectancy violation. Same argument `audio::ambient_oneshots`
    /// makes for the expedition layer.
    pub max_gap: f32,
    /// Distance from the listener the event is placed at. A big room can afford a far-off sound; a
    /// five-cell bunk room cannot, and a drip that reads as coming through a wall is worse than none.
    pub radius: f32,
}

/// The palette, sliced by meaning rather than by index.
const CREAKS: (usize, usize) = (0, 6);
const DRIPS: (usize, usize) = (6, 8);
const CLOCK: (usize, usize) = (8, 9);

/// **What each wing sounds like.**
///
/// Total over `AreaId` with no `_` arm, exactly as `visuals::area_light` is: a new area is then a
/// compile error rather than a room that silently inherits somebody else's sound.
pub fn area_tone(id: AreaId) -> AreaTone {
    match id {
        // The archive keeps time. A clock is the one clip in the palette that reads as *maintained*
        // rather than as decay, which is what a room somebody works in every day should sound like —
        // and it is Farrow's room, the one place in the Site whose job is to remember.
        AreaId::Records => AreaTone { clips: CLOCK, min_gap: 9.0, max_gap: 17.0, radius: 4.0 },

        // The containment wing drips, and it is the busiest tone in the building. Six sealed booths
        // and a coolant loop; the reason to make it the densest is that it is the room the player has
        // most reason to be uneasy in, and the acoustic program's §4 point is that ambience is where
        // a containment readout belongs.
        AreaId::Containment => AreaTone { clips: DRIPS, min_gap: 5.0, max_gap: 11.0, radius: 7.0 },
        // Monitoring watches those cells and shares their plumbing, sparser for being a smaller room.
        AreaId::Monitoring => AreaTone { clips: DRIPS, min_gap: 11.0, max_gap: 21.0, radius: 3.0 },
        // The research wing is wet work too, but slower: this is a room where things are looked at,
        // not held down.
        AreaId::Research => AreaTone { clips: DRIPS, min_gap: 13.0, max_gap: 24.0, radius: 5.5 },

        // The living half creaks. A site recommissioned after years shut is a building settling, and
        // these are the rooms nobody has re-fitted — the fiction the whole dressing pass is built on.
        AreaId::Quarters => AreaTone { clips: CREAKS, min_gap: 8.0, max_gap: 19.0, radius: 3.0 },
        AreaId::Kitchen => AreaTone { clips: CREAKS, min_gap: 9.0, max_gap: 20.0, radius: 3.0 },
        AreaId::Activities => AreaTone { clips: CREAKS, min_gap: 10.0, max_gap: 22.0, radius: 3.0 },

        // The working rooms are quieter than the living ones, which is the inversion worth having:
        // the parts of the Site the Foundation actually re-commissioned are the parts that have been
        // made to behave.
        AreaId::Requisition => AreaTone { clips: CREAKS, min_gap: 15.0, max_gap: 30.0, radius: 4.0 },
        AreaId::Briefing => AreaTone { clips: CREAKS, min_gap: 16.0, max_gap: 32.0, radius: 5.0 },
        AreaId::WarRoom => AreaTone { clips: CREAKS, min_gap: 17.0, max_gap: 34.0, radius: 3.0 },

        // The aperture hall is left ALMOST silent, and that is a decision rather than an omission.
        // Its job is to be the dark room the door glows into — `site67.ron` says so where it declines
        // to dress it — and the aperture is the thing you should be hearing there.
        AreaId::AsyncDoor => AreaTone { clips: CREAKS, min_gap: 24.0, max_gap: 48.0, radius: 8.0 },
        // Corridors carry sound from the rooms either side; giving the spine its own voice would put
        // a creak in the one place the player is always passing through.
        AreaId::Corridor => AreaTone { clips: CREAKS, min_gap: 30.0, max_gap: 60.0, radius: 6.0 },
    }
}

/// Footfalls for everybody actually walking in the hub — **one shared voice**, at their centroid.
///
/// One voice and not one per avatar, for the reason `audio::footsteps` records in full: twelve
/// independent voices is what turned a five-person squad into an army. Density scales sub-linearly so
/// a Site with seven staff crossing it patters faster than one with a lone player, without the rate
/// becoming a machine gun.
///
/// "Walking" is `AvatarGoal` distance, not `Velocity`. Site avatars deliberately have no `Velocity` —
/// `drive_avatars` writes `Transform` directly — and inventing one purely to be heard would put a
/// component on a body for the benefit of the audio layer, which is the wrong way round.
pub fn site_footsteps(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    bus: Res<AudioBus>,
    time: Res<Time>,
    avatars: Query<(&Transform, &AvatarGoal), With<SiteAvatar>>,
    mut timer: Local<f32>,
    mut rng: Local<u32>,
    mut last: Local<usize>,
) {
    let mut centroid = Vec3::ZERO;
    let mut movers = 0usize;
    for (tf, goal) in &avatars {
        if goal.walking(tf, WALKING_EPSILON) {
            centroid += tf.translation;
            movers += 1;
        }
    }
    if movers == 0 {
        // Armed, so the next departure steps on its first frame rather than after half a stride.
        *timer = SITE_STRIDE;
        return;
    }
    let interval = (SITE_STRIDE / (movers as f32).sqrt()).max(SITE_MIN_STRIDE);
    *timer += time.delta_secs();
    if *timer < interval {
        return;
    }
    *timer = 0.0;
    let set = assets.concrete_footsteps();
    let idx = pick_variant(&mut rng, set.len(), &mut last);
    commands.spawn((
        AudioPlayer::new(set[idx].clone()),
        one_shot_spatial(
            centroid / movers as f32,
            SITE_FOOT_VOL * bus.sfx,
            jitter(&mut rng, 0.14),
        ),
    ));
}

/// The wing the player is standing in, making its own noise.
///
/// Anchored on the **player's** avatar rather than on a centroid: room tone is about where *you* are,
/// and averaging in seven staff scattered across the building would place every event in the middle
/// of the Site regardless. `PlayerAvatar` and not `SiteAvatar`, the same distinction
/// `visuals::enter_the_door` documents.
pub fn site_room_tone(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    bus: Res<AudioBus>,
    time: Res<Time>,
    current: Res<CurrentArea>,
    avatars: Query<&Transform, With<PlayerAvatar>>,
    mut timer: Local<f32>,
    mut gap: Local<f32>,
    mut rng: Local<u32>,
    mut last: Local<usize>,
) {
    // Nowhere is silent. Standing on unclaimed floor is a real state and the honest sound for it is
    // none — better than borrowing the last room's voice, which would make the tone lag the player.
    let Some(area) = current.0 else {
        *timer = 0.0;
        return;
    };
    let Ok(tf) = avatars.single() else { return };
    let tone = area_tone(area);

    if *gap <= 0.0 {
        *gap = tone.min_gap;
    }
    *timer += time.delta_secs();
    if *timer < *gap {
        return;
    }
    *timer = 0.0;
    // Re-randomised after every event, never a fixed period — see `AreaTone::max_gap`.
    *gap = tone.min_gap + crate::util::rand01(&mut rng) * (tone.max_gap - tone.min_gap);

    let theta = crate::util::rand01(&mut rng) * std::f32::consts::TAU;
    let at = tf.translation + Vec3::new(theta.cos(), 0.0, theta.sin()) * tone.radius;
    let palette = assets.ambient_oneshots();
    let (lo, hi) = tone.clips;
    let idx = lo + pick_variant(&mut rng, hi - lo, &mut last);
    commands.spawn((
        AudioPlayer::new(palette[idx].clone()),
        one_shot_spatial(at, TONE_VOL * bus.ambience, jitter(&mut rng, 0.08)),
    ));
}

/// Site-67's own acoustic layer. **Windowed-only** — `AppState` does not exist in the harness, and
/// nothing here is anything but cosmetic.
pub struct SiteAudioPlugin;

impl Plugin for SiteAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (site_footsteps, site_room_tone).run_if(in_state(AppState::Site)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every wing draws from a real slice of the palette, and the ranges are sane.
    ///
    /// The indices are hand-written offsets into a nine-clip array; an out-of-range `hi` would panic
    /// at the first event, in a room the player might not visit for an hour.
    #[test]
    fn every_wing_indexes_a_real_slice_of_the_palette_and_a_real_interval() {
        const PALETTE: usize = 9; // `AudioAssets::ambient` is `[Handle; 9]`.
        for id in [
            AreaId::AsyncDoor,
            AreaId::Containment,
            AreaId::Research,
            AreaId::Records,
            AreaId::Requisition,
            AreaId::Briefing,
            AreaId::Quarters,
            AreaId::Kitchen,
            AreaId::Activities,
            AreaId::WarRoom,
            AreaId::Monitoring,
            AreaId::Corridor,
        ] {
            let t = area_tone(id);
            let (lo, hi) = t.clips;
            assert!(lo < hi, "{id:?} draws from an empty slice");
            assert!(hi <= PALETTE, "{id:?} indexes past the {PALETTE}-clip palette");
            assert!(
                t.min_gap > 0.0 && t.min_gap < t.max_gap,
                "{id:?} has a degenerate interval — a zero min would fire every frame and a \
                 min == max is a metronome, which is the tell the range exists to avoid"
            );
            assert!(t.radius > 0.0, "{id:?} would place its events on the listener's head");
        }
    }

    /// The wings are actually distinguishable — this is the whole claim.
    ///
    /// The lighting note's line is *"you can tell which wing you are in from the colour of the air"*;
    /// if every room drew the same clips at the same rate, this module would be decoration with a
    /// per-area signature bolted on.
    #[test]
    fn you_can_tell_the_wings_apart_by_ear() {
        assert_ne!(
            area_tone(AreaId::Records).clips,
            area_tone(AreaId::Containment).clips,
            "the archive and the containment wing must not share a voice"
        );
        assert_ne!(
            area_tone(AreaId::Quarters).clips,
            area_tone(AreaId::Containment).clips,
            "the living half and the working half must not share a voice"
        );
        // Containment is the densest room in the building, and deliberately so.
        for other in [AreaId::Quarters, AreaId::Briefing, AreaId::Corridor, AreaId::AsyncDoor] {
            assert!(
                area_tone(AreaId::Containment).max_gap < area_tone(other).max_gap,
                "containment must be busier than {other:?}"
            );
        }
        // ...and the aperture hall is the quietest of the destinations, because its job is to be the
        // dark room the door glows into.
        assert!(
            area_tone(AreaId::AsyncDoor).min_gap > area_tone(AreaId::Records).min_gap,
            "the ASYNC hall must stay out of the way of the aperture"
        );
    }

    /// Room tone sits under the wind bed, and both sit under the informative layer.
    ///
    /// `docs/2026-08-01-acoustic-program.md` §5: *"music must duck under the informative layer. The
    /// worst measured condition was music-on/sound-off; a score that masks the cues stages 1-3
    /// establish would actively undo them."* The same argument governs a per-room bed.
    #[test]
    fn the_room_never_shouts_over_the_things_that_matter() {
        assert!(TONE_VOL < 0.32, "must sit under the calm-music loop's 0.32");
        assert!(SITE_FOOT_VOL < 0.5, "quieter than the expedition's boots — this room is safe");
    }
}
