//! Game audio: maps discrete gameplay events and continuous states to sound.
//!
//! Most sounds route through one [`Sfx`] message. Gameplay systems in other modules `write` a
//! variant at the exact moment something happens — a unit is selected, a bolt is fired, a bolt
//! bites flesh, an enemy dies — and [`play_sfx`] turns each into a one-shot [`AudioPlayer`] with a
//! per-variant volume and a little random pitch so repeats don't sound machine-stamped. Firing is
//! voice-capped per frame so a whole squad on full-auto reads as a firefight, not a buzzsaw.
//!
//! **World sounds are spatialized.** Every message that names a world position (fire, impacts,
//! deaths) — plus the growl, squitter, and ambient layers — spawns a *spatial* emitter at that
//! point, so it pans left/right and attenuates with distance. A single [`SpatialListener`] tracks
//! the camera's ground focus (see [`sync_listener`]); its ear axis lines up with screen-X, so an
//! off-screen-left growl reads from the left (Grimshaw & Schott 2007, *acoustic ecology of FPS* —
//! sound as an audio beacon; Zotkin et al. 2004, *Rendering Localized Spatial Audio* — pan +
//! distance ⇒ externalized). UI blips and music stay non-spatial (they belong to the player).
//!
//! **One mix bus, with sidechain ducking.** All gains route through an [`AudioBus`] (music /
//! ambience / sfx / ui groups + a live duck envelope) so relative levels are tuned in one place, and
//! loud world events automatically dip the music+ambience beds so feedback punches through — then
//! the beds recover (Böttcher & Serafin — buses/HDR; Nacke/Grimshaw 2010 — music-on/SFX-off is the
//! *worst* condition, so never let the bed drown the feedback).
//!
//! Continuous layers are driven locally instead of by messages, because each needs state the
//! emitting site doesn't keep: per-unit carpet footsteps (paced off [`Velocity`]), a monster-growl
//! stinger fired on the false→true edge of an enemy entering sight range (round-robined over several
//! growls so it never sounds stamped), a sparse randomized **ambient one-shot** layer (creaks/drips
//! scattered around the squad — a living space, not a bare loop), and an **adaptive music** layer
//! that crossfades a calm↔combat pair on a continuous *threat scalar* (count × proximity of visible
//! hostiles) rather than a hard cut on a boolean (Khan et al. 2023 — bind gain to a game variable;
//! Kaushik 2025 — layer, don't switch). A backrooms wind bed loops underneath the whole time.
//!
//! Assets live in `assets/audio/**` and are all Ogg Vorbis, so Bevy's default audio decoder plays
//! them with no extra Cargo features (one decode path — no wav/mp3 feature flags).

use std::collections::{HashMap, HashSet};

use bevy::audio::{DefaultSpatialScale, GlobalVolume, SpatialScale, Volume};
use bevy::prelude::*;
use bevy::time::Real;

use crate::crab::Crab;
use crate::dungeon::Dungeon;
use crate::enemy::{Enemy, Hostile, SmileyState};
use crate::fog::FogGrid;
use crate::squad::{Unit, Velocity};
use crate::util::{next_u32, rand01, smoothstep};

/// Looping-bed volumes and the mixing headroom for one-shots. Ambience and footsteps sit low so
/// the foreground (weapons, gore, growls) reads clearly over them.
const WIND_VOL: f32 = 0.22;
const MUSIC_VOL: f32 = 0.32;
const FOOT_VOL: f32 = 0.08;
/// UI / command blips (select, deselect, move order…). Deliberately way under the world sounds — a
/// faint tick you feel more than hear, so a fidgety player isn't machine-gunned with pings.
const UI_VOL: f32 = 0.12;

/// Master volume — every sound in the game is multiplied by this (bevy `GlobalVolume`). 1.0 = full;
/// drop it (e.g. 0.15) to keep the game quiet in the background when something else is playing.
const MASTER_VOLUME: f32 = 1.0;

/// World-units → audio-units scale for spatial attenuation/panning. The listener sits on the ground
/// at the camera focus, so this stretches the ~5–34-unit viewport into the audible near field: lower
/// ⇒ sounds carry farther (gentler rolloff). Empirical — tuned by ear, not derived (rodio's rolloff
/// curve isn't a documented constant). See [`sync_listener`].
const SPATIAL_SCALE: f32 = 0.35;
/// Ear separation (world units, along the listener's local X = screen-right) for stereo pan width.
/// Bigger ⇒ wider stereo image. Empirical.
const LISTENER_GAP: f32 = 3.0;

/// Seconds for the music crossfade to travel the full calm↔combat span. Runs on **real** time, so it
/// neither freezes when the sim is paused nor races at high game speed (same rationale as the camera).
const FADE_SECS: f32 = 1.5;
/// Threat-scalar proximity curve (mirrors the smiley's `SIGHT_NEAR`/`SIGHT_FAR` grin ramp): a visible
/// hostile this close counts as full-intensity combat; one at/beyond `THREAT_FAR` adds ~nothing. The
/// music intensity is the max over all visible hostiles of `smoothstep(FAR, NEAR, nearest_unit_dist)`.
const THREAT_NEAR: f32 = 3.0;
const THREAT_FAR: f32 = 14.0;

/// Sidechain ducking: each loud world event pushes the duck envelope up by `DUCK_ATTACK` (capped at
/// 1); it decays back toward 0 at `DUCK_DECAY`/s (≈0.4 s after the last hit). At full duck the music
/// and ambience beds drop to `1 − DUCK_DEPTH` of their level, so gunfire/growls/deaths cut through
/// without the bed being permanently quieter (the fix for "too quiet under action").
const DUCK_ATTACK: f32 = 0.3;
const DUCK_DECAY: f32 = 2.5;
const DUCK_DEPTH: f32 = 0.45;

/// Randomized ambient one-shot layer: a creak/drip scattered every `AMBIENT_MIN_GAP`–`AMBIENT_MAX_GAP`
/// seconds, placed `AMBIENT_RADIUS` out from the squad on a random bearing so it reads as coming from
/// somewhere in the dungeon rather than a metronome on the player. Kept low, under the wind bed.
const AMBIENT_MIN_GAP: f32 = 7.0;
const AMBIENT_MAX_GAP: f32 = 18.0;
const AMBIENT_RADIUS: f32 = 11.0;
const AMBIENT_VOL: f32 = 0.4;

/// Seconds between footfalls with a *single* unit walking. The interval scales down linearly with
/// the number of movers (see `footsteps`), so a full squad patters ~5× faster and the sound
/// audibly thins as members die.
const STRIDE: f32 = 0.5;
/// Floor on the footfall interval, so even a full squad's patter never machine-guns into a crowd —
/// caps a full five at ~4.5 steps/s (was 0.12 → ~8/s, which read as a mob).
const MIN_STRIDE: f32 = 0.22;
/// A unit is "walking" (and so contributes a footfall) once its planar speed clears this. Well
/// under the `UNIT_SPEED = 6.0` cruise, but above ORCA jitter so a settled blob stays quiet.
const FOOT_MIN_SPEED: f32 = 0.6;

/// Crab-swarm "squitter": ONE shared throttled voice whose cadence scales with how many crabs are
/// visibly near the squad — same density-throttle discipline as `footsteps` (never one voice per crab,
/// which is what turned the squad's footfalls into a phantom crowd). Volume sits low, under the action.
const SQUITTER_VOL: f32 = 0.30;
/// Seconds between skitters with a single near crab; divided by the near-crab count and floored.
const SQUITTER_STRIDE: f32 = 0.45;
const SQUITTER_MIN_STRIDE: f32 = 0.09;
/// A crab counts toward the squitter density only within this planar distance of some unit.
const SQUITTER_RANGE: f32 = 10.0;
/// Per-frame cap on muzzle blasts: a five-unit squad on full-auto must not stack into a solid wall
/// of identical shots. Extra triggers in the same frame are dropped (pitch jitter hides it).
const MAX_FIRE_VOICES: usize = 4;

/// Planar distance (world units, XZ) at which a visible enemy triggers its one-shot growl. Mirrors
/// `enemy::SIGHT_NEAR` — the range at which the smiley face grins on prey — so the sound lands as
/// the grin widens.
const GROWL_RANGE: f32 = 3.0;
const GROWL_VOL: f32 = 0.6;

/// The watcher's mask-off reveal: a sharp horror sting fired the instant it flips from concealed
/// (`Watching`/`Scared`) to `Unleashing`. Percussiveness with a sharp attack spikes skin conductance
/// *below the conscious threshold* (van der Zwaag, Westerink & van den Broek 2011) — exactly the
/// subconscious startle horror wants, timed to the frame the concealment cracks (see `enemy::SmileyMood`).
/// Loud, and it ducks the whole bed to full so the reveal lands in the clear.
const WATCHER_REVEAL_VOL: f32 = 0.85;

/// A discrete sound request. Gameplay systems elsewhere write these; [`play_sfx`] consumes them.
///
/// World variants carry the world-space [`Vec3`] where the event happened, so [`play_sfx`] can spawn
/// a *spatial* emitter there — panned and distance-attenuated relative to the listener. UI variants
/// carry no position (they're non-spatial, centred on the player). Encoding it in the type means a
/// world sound *cannot* be emitted without a position — there's no "forgot the position" degraded path.
#[derive(Message, Clone, Copy)]
pub enum Sfx {
    /// A move order was issued to at least one unit. (UI — non-spatial.)
    MoveOrder,
    /// A move click had nowhere reachable to go. (UI — non-spatial.)
    Invalid,
    /// One laser bolt left a muzzle, at the muzzle.
    Fire(Vec3),
    /// A bolt struck a wall, at the impact point.
    ImpactWall(Vec3),
    /// A bolt struck an enemy, at the hit point.
    ImpactFlesh(Vec3),
    /// An enemy was killed, at its position.
    EnemyDeath(Vec3),
    /// A squad unit was killed, at its position.
    UnitDeath(Vec3),
}

impl Sfx {
    /// True for the loud world events that duck the music+ambience beds (sidechain). Fire, flesh
    /// hits, and deaths are the transients feedback needs to punch through; UI blips and wall pings
    /// don't warrant dipping the bed.
    fn ducks(&self) -> bool {
        matches!(
            self,
            Sfx::Fire(_) | Sfx::ImpactFlesh(_) | Sfx::EnemyDeath(_) | Sfx::UnitDeath(_)
        )
    }
}

/// Handles for every clip, loaded once at startup.
#[derive(Resource)]
struct AudioAssets {
    move_order: Handle<AudioSource>,
    invalid: Handle<AudioSource>,
    fire: Handle<AudioSource>,
    wall: Handle<AudioSource>,
    /// Wet splat for a bolt biting flesh (per hit).
    splat: Handle<AudioSource>,
    /// Squelch for an enemy bursting.
    squelch: Handle<AudioSource>,
    /// Wet crunch for a squad unit being crushed to gibs.
    crunch: Handle<AudioSource>,
    /// Bone snap layered over the crunch for extra juice.
    bone_snap: Handle<AudioSource>,
    /// Monster growls — round-robined (no immediate repeat) so a repeatedly-sighted enemy doesn't
    /// stamp the same clip (Böttcher & Serafin — sample variation is the first rung against repetition).
    growls: [Handle<AudioSource>; 4],
    /// Sharp horror sting for the watcher's mask-off reveal (see `watcher_stinger`).
    watcher_reveal: Handle<AudioSource>,
    /// Dry insectoid chitter for the crab swarm (shared throttled voice, see `crab_squitter`).
    squitter: Handle<AudioSource>,
    footsteps: [Handle<AudioSource>; 4],
    /// Sparse ambient one-shots (door creaks, water drips, a clock tick) for the randomized layer.
    ambient: [Handle<AudioSource>; 6],
    wind: Handle<AudioSource>,
    music_calm: Handle<AudioSource>,
    music_combat: Handle<AudioSource>,
}

/// Mix bus: relative group gains tuned in one place, plus a live sidechain `duck` envelope. Every
/// one-shot multiplies its per-clip base by its group gain at spawn; the forever-alive beds multiply
/// by their group gain × the duck factor each frame. Master + mute stay on `GlobalVolume`.
#[derive(Resource)]
struct AudioBus {
    music: f32,
    ambience: f32,
    sfx: f32,
    ui: f32,
    /// 0 = no duck … 1 = full duck. Bumped by loud events, decays each frame.
    duck: f32,
}

impl Default for AudioBus {
    fn default() -> Self {
        // Group gains start at unity — the per-clip constants are already balanced; the bus exists so
        // that balance is tuneable in one place and to carry the duck envelope.
        Self {
            music: 1.0,
            ambience: 1.0,
            sfx: 1.0,
            ui: 1.0,
            duck: 0.0,
        }
    }
}

/// Adaptive-music state: `intensity` ∈ [0, 1] is the crossfade position (0 = calm, 1 = full combat),
/// eased toward a threat target each frame. The two loops both play forever (see `load_audio`); we
/// only modulate their gains — no despawn/spawn cut.
#[derive(Resource)]
struct MusicState {
    intensity: f32,
}

/// Marker: the always-on backrooms wind bed. Its gain is driven each frame (× ambience × duck ×
/// master) so it ducks under action and mutes on a live alt-tab — `GlobalVolume` alone only affects
/// sounds at *birth*, not a forever-alive sink.
#[derive(Component)]
struct WindBed;
/// Marker: the calm music loop (audible when `intensity` = 0).
#[derive(Component)]
struct CalmMusic;
/// Marker: the combat music loop (audible when `intensity` = 1). Plays silently underneath from the
/// start so the crossfade only has to move gains, never start a cold track.
#[derive(Component)]
struct CombatMusic;

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Sfx>()
            .insert_resource(GlobalVolume::new(Volume::Linear(MASTER_VOLUME)))
            // World-units → audio-units for every spatial emitter that doesn't override it.
            .insert_resource(DefaultSpatialScale(SpatialScale::new(SPATIAL_SCALE)))
            .init_resource::<AudioBus>()
            .add_systems(Startup, load_audio)
            .add_systems(
                Update,
                (
                    sync_listener,
                    update_duck,
                    play_sfx,
                    footsteps,
                    crab_squitter,
                    growl_stinger,
                    watcher_stinger,
                    ambient_oneshots,
                    update_music,
                    mute_when_background,
                ),
            );
    }
}

/// Silence the whole mix whenever the game is in the **background** — its window unfocused (alt-tabbed,
/// another Space, minimised) or absent entirely (a headless run: the sim harness, a background/CI
/// session, or a devshot capture). Restores `MASTER_VOLUME` the instant the window regains focus. One
/// cheap resource write per frame; no per-sound bookkeeping. This mutes one-shots directly (they read
/// `GlobalVolume` at birth); the forever-alive beds (wind + music) read it each frame in `update_music`
/// so they mute on a live alt-tab too. This is why running the game in the background — as this project
/// does for `devshot` screenshots — makes no noise.
fn mute_when_background(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut volume: ResMut<GlobalVolume>,
) {
    // No primary window at all (headless) ⇒ treat as background ⇒ muted.
    let focused = windows.iter().next().is_some_and(|w| w.focused);
    volume.volume = Volume::Linear(if focused { MASTER_VOLUME } else { 0.0 });
}

/// Load every handle, spawn the spatial listener, start the always-on wind bed, and start BOTH music
/// loops (combat silent) so the adaptive crossfade only ever moves gains.
fn load_audio(mut commands: Commands, assets: Res<AssetServer>) {
    let a = AudioAssets {
        move_order: assets.load("audio/ui/move_order.ogg"),
        invalid: assets.load("audio/ui/invalid.ogg"),
        fire: assets.load("audio/weapon/fire.ogg"),
        wall: assets.load("audio/impact/wall.ogg"),
        splat: assets.load("audio/impact/splat.ogg"),
        squelch: assets.load("audio/impact/squelch.ogg"),
        crunch: assets.load("audio/impact/crunch.ogg"),
        bone_snap: assets.load("audio/impact/bone_snap.ogg"),
        growls: [
            assets.load("audio/enemy/growl_1.ogg"),
            assets.load("audio/enemy/growl_2.ogg"),
            assets.load("audio/enemy/growl_3.ogg"),
            assets.load("audio/enemy/growl_4.ogg"),
        ],
            watcher_reveal: assets.load("audio/enemy/watcher_reveal.ogg"),
        squitter: assets.load("audio/enemy/squitter.ogg"),
        footsteps: [
            assets.load("audio/foot/carpet_1.ogg"),
            assets.load("audio/foot/carpet_2.ogg"),
            assets.load("audio/foot/carpet_3.ogg"),
            assets.load("audio/foot/carpet_4.ogg"),
        ],
        ambient: [
            assets.load("audio/ambience/oneshot/creak_1.ogg"),
            assets.load("audio/ambience/oneshot/creak_2.ogg"),
            assets.load("audio/ambience/oneshot/creak_3.ogg"),
            assets.load("audio/ambience/oneshot/drip_1.ogg"),
            assets.load("audio/ambience/oneshot/drip_2.ogg"),
            assets.load("audio/ambience/oneshot/clock.ogg"),
        ],
        wind: assets.load("audio/ambience/wind.ogg"),
        music_calm: assets.load("audio/music/calm.ogg"),
        music_combat: assets.load("audio/music/combat.ogg"),
    };

    // One spatial listener for the whole game. `sync_listener` parks it on the ground under the
    // camera focus each frame; the ears sit on its local X (= screen-right), giving left/right pan.
    commands.spawn((SpatialListener::new(LISTENER_GAP), Transform::default()));

    // Backrooms wind bed — loops forever underneath everything.
    commands.spawn((AudioPlayer::new(a.wind.clone()), looped(WIND_VOL), WindBed));

    // Both music loops run from the start; `update_music` crossfades their gains on the threat scalar.
    // Combat starts silent (but *playing*, not paused) so it's phase-ready the instant a fight opens.
    commands.spawn((
        AudioPlayer::new(a.music_calm.clone()),
        looped(MUSIC_VOL),
        CalmMusic,
    ));
    commands.spawn((
        AudioPlayer::new(a.music_combat.clone()),
        looped(0.0),
        CombatMusic,
    ));
    commands.insert_resource(MusicState { intensity: 0.0 });
    commands.insert_resource(a);
}

/// Park the spatial listener on the ground point under the camera focus, rotated with the camera, so
/// its local X (the ear axis) equals screen-right and the plane it hears is the plane the player sees.
///
/// The iso camera sits ~20 units up at `focus + ISO_OFFSET`; a listener *on* the camera would hear
/// everything ~20 units away (attenuation crushed, pan biased toward screen-forward). Recovering the
/// ground focus as `camera_translation − ISO_OFFSET` puts "screen centre" at zero distance instead.
/// `Single` cleanly skips the system when there's no unique camera/listener (headless spin-up, first
/// frame) — no panic, no unwrap.
fn sync_listener(
    camera: Single<&GlobalTransform, With<Camera3d>>,
    listener: Single<&mut Transform, (With<SpatialListener>, Without<Camera3d>)>,
) {
    let cam = camera.into_inner();
    let mut tf = listener.into_inner();
    tf.translation = cam.translation() - crate::camera::ISO_OFFSET;
    tf.rotation = cam.rotation();
}

/// Advance the sidechain duck envelope: decay it toward 0 on real time, then bump it for each loud
/// world event this frame (see [`Sfx::ducks`]). Reads the same [`Sfx`] stream as [`play_sfx`] (each
/// `MessageReader` has its own cursor). Growls bump it too, directly in [`growl_stinger`].
fn update_duck(time: Res<Time<Real>>, mut msgs: MessageReader<Sfx>, mut bus: ResMut<AudioBus>) {
    bus.duck = (bus.duck - DUCK_DECAY * time.delta_secs()).max(0.0);
    for sfx in msgs.read() {
        if sfx.ducks() {
            bus.duck = (bus.duck + DUCK_ATTACK).min(1.0);
        }
    }
}

/// Play one clip per queued [`Sfx`], voice-capping the fire spam and jittering pitch so repeats
/// don't sound stamped. World variants spawn *spatial* emitters at their event position; UI variants
/// stay non-spatial. Per-clip volumes route through the mix bus's sfx/ui group gains.
fn play_sfx(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    bus: Res<AudioBus>,
    mut msgs: MessageReader<Sfx>,
    mut rng: Local<u32>,
) {
    let mut fire_voices = 0usize;
    for sfx in msgs.read() {
        match sfx {
            // UI blips — non-spatial, centred on the player.
            Sfx::MoveOrder => {
                commands.spawn((
                    AudioPlayer::new(assets.move_order.clone()),
                    one_shot(UI_VOL * bus.ui, jitter(&mut rng, 0.05)),
                ));
            }
            Sfx::Invalid => {
                commands.spawn((
                    AudioPlayer::new(assets.invalid.clone()),
                    one_shot(UI_VOL * bus.ui, jitter(&mut rng, 0.03)),
                ));
            }
            // World sounds — spatialized at the event point.
            Sfx::Fire(pos) => {
                // Cap concurrent muzzle blasts this frame; a whole squad firing stays a firefight.
                if fire_voices >= MAX_FIRE_VOICES {
                    continue;
                }
                fire_voices += 1;
                commands.spawn((
                    AudioPlayer::new(assets.fire.clone()),
                    one_shot_spatial(*pos, 0.32 * bus.sfx, jitter(&mut rng, 0.15)),
                ));
            }
            Sfx::ImpactWall(pos) => {
                commands.spawn((
                    AudioPlayer::new(assets.wall.clone()),
                    one_shot_spatial(*pos, 0.4 * bus.sfx, jitter(&mut rng, 0.12)),
                ));
            }
            // Flesh hits, enemy bursts, and unit crunches are gory now (see `gore`).
            Sfx::ImpactFlesh(pos) => {
                commands.spawn((
                    AudioPlayer::new(assets.splat.clone()),
                    one_shot_spatial(*pos, 0.55 * bus.sfx, jitter(&mut rng, 0.12)),
                ));
            }
            Sfx::EnemyDeath(pos) => {
                commands.spawn((
                    AudioPlayer::new(assets.squelch.clone()),
                    one_shot_spatial(*pos, 0.7 * bus.sfx, jitter(&mut rng, 0.08)),
                ));
            }
            Sfx::UnitDeath(pos) => {
                commands.spawn((
                    AudioPlayer::new(assets.crunch.clone()),
                    one_shot_spatial(*pos, 0.85 * bus.sfx, jitter(&mut rng, 0.06)),
                ));
                // A crunched unit layers a bone snap over the wet crunch, same spot.
                commands.spawn((
                    AudioPlayer::new(assets.bone_snap.clone()),
                    one_shot_spatial(*pos, 0.7 * bus.sfx, jitter(&mut rng, 0.1)),
                ));
            }
        }
    }
}

/// Squad footfalls from a single shared voice (never overlapping — that's what turned five units
/// into an army). Density scales linearly with the number of units actually walking, so a full
/// squad patters ~5× faster than a lone survivor and the sound audibly thins as members die. Kept
/// quiet so it's floor texture under the action. Non-spatial: the squad is what the camera frames,
/// so its footfalls belong at the centre rather than smeared across the stereo field.
fn footsteps(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    bus: Res<AudioBus>,
    time: Res<Time>,
    units: Query<&Velocity, With<Unit>>,
    mut timer: Local<f32>,
    mut rng: Local<u32>,
    mut last: Local<usize>,
) {
    let movers = units
        .iter()
        .filter(|v| v.0.length() >= FOOT_MIN_SPEED)
        .count();
    if movers == 0 {
        *timer = STRIDE; // idle → armed, so the next departure steps on its first frame
        return;
    }
    // More boots on the ground ⇒ proportionally shorter gap between steps, floored so it never
    // machine-guns. ~0.5s/step for one survivor down to ~0.22s for a full five.
    let interval = (STRIDE / movers as f32).max(MIN_STRIDE);
    *timer += time.delta_secs();
    if *timer >= interval {
        *timer = 0.0;
        let idx = pick_variant(&mut rng, assets.footsteps.len(), &mut last);
        commands.spawn((
            AudioPlayer::new(assets.footsteps[idx].clone()),
            one_shot(FOOT_VOL * bus.sfx, jitter(&mut rng, 0.08)),
        ));
    }
}

/// Crab-swarm skitter from a single shared voice (never overlapping per-crab — the same fix as
/// `footsteps`). Density scales with the number of crabs currently near the squad, so a fresh
/// infestation chitters densely and the sound thins as the swarm is culled; silent when none are near.
/// Emitted spatially at the centroid of the near crabs, so the swarm skitter comes from where the
/// swarm actually is.
fn crab_squitter(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    bus: Res<AudioBus>,
    time: Res<Time>,
    crabs: Query<&Transform, With<Crab>>,
    units: Query<&Transform, (With<Unit>, Without<Crab>)>,
    mut timer: Local<f32>,
    mut rng: Local<u32>,
) {
    // Count crabs within SQUITTER_RANGE (planar) of any unit — the audible swarm — and sum their
    // positions for a centroid to place the shared voice.
    let mut near = 0usize;
    let mut sum = Vec3::ZERO;
    for c in &crabs {
        let close = units
            .iter()
            .any(|u| (c.translation.xz() - u.translation.xz()).length() <= SQUITTER_RANGE);
        if close {
            near += 1;
            sum += c.translation;
        }
    }
    if near == 0 {
        *timer = SQUITTER_STRIDE; // armed, so the next crab that closes in skitters immediately
        return;
    }
    // More crabs ⇒ denser skittering, floored so it never machine-guns into mush (as with footsteps).
    let interval = (SQUITTER_STRIDE / near as f32).max(SQUITTER_MIN_STRIDE);
    *timer += time.delta_secs();
    if *timer >= interval {
        *timer = 0.0;
        let centroid = sum / near as f32;
        commands.spawn((
            AudioPlayer::new(assets.squitter.clone()),
            one_shot_spatial(centroid, SQUITTER_VOL * bus.sfx, jitter(&mut rng, 0.12)),
        ));
    }
}

/// Growl when a *visible* enemy first crosses inside [`GROWL_RANGE`] of any unit. Edge-triggered per
/// enemy so it stings once on sighting, not every frame, and re-arms when the enemy leaves range.
/// Spatialized at the growling enemy, round-robined over the growl set (no immediate repeat), and it
/// ducks the beds — a fresh growl is a loud stinger that should punch through.
fn growl_stinger(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    mut bus: ResMut<AudioBus>,
    dungeon: Res<Dungeon>,
    fog: Res<FogGrid>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
    units: Query<&Transform, (With<Unit>, Without<Enemy>)>,
    mut near: Local<HashMap<Entity, bool>>,
    mut rng: Local<u32>,
    mut last: Local<usize>,
) {
    let live: HashSet<Entity> = enemies.iter().map(|(e, _)| e).collect();

    for (entity, etf) in &enemies {
        // Nearest unit on the ground plane (enemy capsule and unit sit at different heights).
        let mut nearest = f32::MAX;
        for utf in &units {
            nearest = nearest.min((etf.translation.xz() - utf.translation.xz()).length());
        }
        let visible = fog.visible_at(dungeon.world_to_cell(etf.translation));
        let is_near = visible && nearest <= GROWL_RANGE;
        let was_near = near.insert(entity, is_near).unwrap_or(false);
        if is_near && !was_near {
            let idx = pick_variant(&mut rng, assets.growls.len(), &mut last);
            commands.spawn((
                AudioPlayer::new(assets.growls[idx].clone()),
                one_shot_spatial(etf.translation, GROWL_VOL * bus.sfx, jitter(&mut rng, 0.1)),
            ));
            bus.duck = (bus.duck + DUCK_ATTACK).min(1.0);
        }
    }

    near.retain(|e, _| live.contains(e));
}

/// Percussive stinger the instant a watcher drops its mask — the `is_angry` false→true edge
/// (`Watching`/`Scared` → `Unleashing`). Edge-tracked per watcher so it hits once on the reveal, not
/// every frame it stays angry, and re-arms when it calms down. Spatialized at the watcher and ducks the
/// bed to full: this is the subconscious-startle beat (percussiveness → skin conductance, van der Zwaag
/// et al. 2011), timed to the concealment cracking. Only the smiley boss carries `SmileyState`, so the
/// query naturally filters to watchers.
fn watcher_stinger(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    mut bus: ResMut<AudioBus>,
    watchers: Query<(Entity, &Transform, &SmileyState)>,
    mut angry: Local<HashMap<Entity, bool>>,
    mut rng: Local<u32>,
) {
    let live: HashSet<Entity> = watchers.iter().map(|(e, _, _)| e).collect();

    for (entity, tf, state) in &watchers {
        let is_angry = state.is_angry();
        let was_angry = angry.insert(entity, is_angry).unwrap_or(false);
        if is_angry && !was_angry {
            commands.spawn((
                AudioPlayer::new(assets.watcher_reveal.clone()),
                one_shot_spatial(tf.translation, WATCHER_REVEAL_VOL * bus.sfx, jitter(&mut rng, 0.05)),
            ));
            // The mask cracking gets the whole bed out of the way for the reveal.
            bus.duck = 1.0;
        }
    }

    angry.retain(|e, _| live.contains(e));
}

/// Sparse randomized ambient one-shots (creaks, drips, a distant clock) scattered around the squad
/// over the wind bed, so the dungeon reads as a living space rather than a looping WAV. The interval
/// is re-randomized after every event and the clip is round-robined, so it never becomes a tell; an
/// irregular creak from off in the dark is a low-grade expectancy violation (unease). Placed
/// `AMBIENT_RADIUS` out from the squad centroid on a random bearing, spatialized. Silent with no squad.
fn ambient_oneshots(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    bus: Res<AudioBus>,
    time: Res<Time>,
    units: Query<&Transform, With<Unit>>,
    mut timer: Local<f32>,
    mut gap: Local<f32>,
    mut rng: Local<u32>,
    mut last: Local<usize>,
) {
    // Squad centroid to scatter events around; nothing to anchor to ⇒ stay silent.
    let mut sum = Vec3::ZERO;
    let mut n = 0u32;
    for tf in &units {
        sum += tf.translation;
        n += 1;
    }
    if n == 0 {
        *timer = 0.0;
        return;
    }
    // First run (gap still 0): arm a randomized interval.
    if *gap <= 0.0 {
        *gap = AMBIENT_MIN_GAP + rand01(&mut rng) * (AMBIENT_MAX_GAP - AMBIENT_MIN_GAP);
    }
    *timer += time.delta_secs();
    if *timer < *gap {
        return;
    }
    *timer = 0.0;
    *gap = AMBIENT_MIN_GAP + rand01(&mut rng) * (AMBIENT_MAX_GAP - AMBIENT_MIN_GAP);

    let centroid = sum / n as f32;
    // Random bearing on the ground plane, AMBIENT_RADIUS out.
    let theta = rand01(&mut rng) * std::f32::consts::TAU;
    let pos = centroid + Vec3::new(theta.cos(), 0.0, theta.sin()) * AMBIENT_RADIUS;
    let idx = pick_variant(&mut rng, assets.ambient.len(), &mut last);
    commands.spawn((
        AudioPlayer::new(assets.ambient[idx].clone()),
        one_shot_spatial(pos, AMBIENT_VOL * bus.ambience, jitter(&mut rng, 0.06)),
    ));
}

/// Adaptive music: crossfade the calm↔combat pair on a continuous **threat scalar** instead of a hard
/// cut on a boolean. Threat = the max over every *visible* hostile (excluding an inert Nest) of a
/// proximity ramp to the nearest unit, so the music breathes with how close a fight is, not just
/// whether one exists. Both loops stay alive; we only move their gains (× music group × duck × master).
/// The wind bed rides the same duck × master here so the whole bed layer dips together under action and
/// mutes on a live alt-tab (`set_volume` bypasses `GlobalVolume`, which is applied only at a sink's birth).
fn update_music(
    time: Res<Time<Real>>,
    bus: Res<AudioBus>,
    dungeon: Res<Dungeon>,
    fog: Res<FogGrid>,
    gv: Res<GlobalVolume>,
    // A cleared-but-standing Nest is `Hostile` (siege-killable) yet not a live threat, so exclude it —
    // otherwise the combat track latches on forever whenever an inert nest sits in the squad's LOS.
    enemies: Query<&Transform, (With<Hostile>, Without<crate::nest::Nest>)>,
    units: Query<&Transform, With<Unit>>,
    mut state: ResMut<MusicState>,
    mut wind: Query<&mut AudioSink, (With<WindBed>, Without<CalmMusic>, Without<CombatMusic>)>,
    mut calm: Query<&mut AudioSink, (With<CalmMusic>, Without<CombatMusic>)>,
    mut combat: Query<&mut AudioSink, (With<CombatMusic>, Without<CalmMusic>)>,
) {
    // Continuous threat scalar: for each visible hostile, weight by proximity to the nearest unit
    // (1 at THREAT_NEAR, 0 past THREAT_FAR); the loudest single threat sets the intensity target.
    let mut target = 0.0f32;
    for etf in &enemies {
        if !fog.visible_at(dungeon.world_to_cell(etf.translation)) {
            continue;
        }
        let mut nearest = f32::MAX;
        for utf in &units {
            nearest = nearest.min((etf.translation.xz() - utf.translation.xz()).length());
        }
        target = target.max(smoothstep(THREAT_FAR, THREAT_NEAR, nearest));
    }

    // Ease intensity toward the target on REAL time, so the fade neither freezes when paused nor
    // races at high game speed (matches the camera's real-time controls).
    let step = time.delta_secs() / FADE_SECS;
    state.intensity = if state.intensity < target {
        (state.intensity + step).min(target)
    } else {
        (state.intensity - step).max(target)
    };

    // Equal-power crossfade (constant perceived loudness across the blend), ducked and muted.
    let angle = state.intensity * std::f32::consts::FRAC_PI_2;
    let duck_factor = 1.0 - bus.duck * DUCK_DEPTH;
    let calm_g = Volume::Linear(angle.cos() * MUSIC_VOL * bus.music * duck_factor) * gv.volume;
    let combat_g = Volume::Linear(angle.sin() * MUSIC_VOL * bus.music * duck_factor) * gv.volume;
    let wind_g = Volume::Linear(WIND_VOL * bus.ambience * duck_factor) * gv.volume;

    // Guard each: the `AudioSink` component only exists once the sink is created (a frame or two after
    // spawn), so early frames simply no-op — no unwrap, no panic.
    if let Ok(mut sink) = calm.single_mut() {
        sink.set_volume(calm_g);
    }
    if let Ok(mut sink) = combat.single_mut() {
        sink.set_volume(combat_g);
    }
    if let Ok(mut sink) = wind.single_mut() {
        sink.set_volume(wind_g);
    }
}

/// One-shot playback: play once at `vol`/`speed`, then despawn the entity. Non-spatial (UI / squad).
fn one_shot(vol: f32, speed: f32) -> PlaybackSettings {
    let mut s = PlaybackSettings::DESPAWN;
    s.volume = Volume::Linear(vol);
    s.speed = speed;
    s
}

/// Spatial one-shot: same as [`one_shot`] but positioned in the world so it pans + attenuates. Returns
/// the `Transform` alongside the settings so a caller spawns `(AudioPlayer, one_shot_spatial(..))`.
/// The emitter reads its `GlobalTransform` after transform propagation (Bevy runs audio playback
/// `after(TransformSystems::Propagate)`), so its position is correct on the very first frame.
fn one_shot_spatial(pos: Vec3, vol: f32, speed: f32) -> (PlaybackSettings, Transform) {
    let mut s = PlaybackSettings::DESPAWN.with_spatial(true);
    s.volume = Volume::Linear(vol);
    s.speed = speed;
    (s, Transform::from_translation(pos))
}

/// Looping playback at a fixed volume (beds and music).
fn looped(vol: f32) -> PlaybackSettings {
    let mut s = PlaybackSettings::LOOP;
    s.volume = Volume::Linear(vol);
    s
}

/// A playback-speed multiplier of `1.0 ± amount`, so repeated one-shots don't sound identical.
fn jitter(state: &mut u32, amount: f32) -> f32 {
    1.0 + (rand01(state) * 2.0 - 1.0) * amount
}

/// Round-robin pick over `len` variants with **no immediate repeat**: never returns the same index
/// twice in a row (unless there's only one), so a repeatedly-triggered sound cycles instead of
/// stamping one clip. `last` is the caller's running memory of the previous pick. Panic-free.
fn pick_variant(rng: &mut u32, len: usize, last: &mut usize) -> usize {
    if len <= 1 {
        *last = 0;
        return 0;
    }
    let mut idx = (next_u32(rng) as usize) % len;
    if idx == *last {
        idx = (idx + 1) % len;
    }
    *last = idx;
    idx
}
