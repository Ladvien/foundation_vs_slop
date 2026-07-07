//! Game audio: maps discrete gameplay events and continuous states to sound.
//!
//! Most sounds route through one [`Sfx`] message. Gameplay systems in other modules `write` a
//! variant at the exact moment something happens — a unit is selected, a bolt is fired, a bolt
//! bites flesh, an enemy dies — and [`play_sfx`] turns each into a one-shot [`AudioPlayer`] with a
//! per-variant volume and a little random pitch so repeats don't sound machine-stamped. Firing is
//! voice-capped per frame so a whole squad on full-auto reads as a firefight, not a buzzsaw.
//!
//! Three continuous layers are driven locally instead of by messages, because each needs state the
//! emitting site doesn't keep: per-unit carpet footsteps (paced off [`Velocity`]), a monster-growl
//! stinger fired on the false→true edge of an enemy entering sight range, and a calm↔combat music
//! swap keyed on whether any enemy sits in the squad's fog line of sight. A backrooms wind bed
//! loops underneath the whole time.
//!
//! Assets live in `assets/audio/**` and are all Ogg Vorbis, so Bevy's default audio decoder plays
//! them with no extra Cargo features (one decode path — no wav/mp3 feature flags).

use std::collections::{HashMap, HashSet};

use bevy::audio::{GlobalVolume, Volume};
use bevy::prelude::*;

use crate::crab::Crab;
use crate::dungeon::Dungeon;
use crate::enemy::{Enemy, Hostile};
use crate::fog::FogGrid;
use crate::squad::{Unit, Velocity};
use crate::util::{next_u32, rand01};

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

/// A discrete sound request. Gameplay systems elsewhere write these; [`play_sfx`] consumes them.
#[derive(Message, Clone, Copy)]
pub enum Sfx {
    /// A move order was issued to at least one unit.
    MoveOrder,
    /// A move click had nowhere reachable to go.
    Invalid,
    /// One laser bolt left a muzzle.
    Fire,
    /// A bolt struck a wall.
    ImpactWall,
    /// A bolt struck an enemy.
    ImpactFlesh,
    /// An enemy was killed.
    EnemyDeath,
    /// A squad unit was killed.
    UnitDeath,
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
    growl: Handle<AudioSource>,
    /// Dry insectoid chitter for the crab swarm (shared throttled voice, see `crab_squitter`).
    squitter: Handle<AudioSource>,
    footsteps: [Handle<AudioSource>; 4],
    wind: Handle<AudioSource>,
    music_calm: Handle<AudioSource>,
    music_combat: Handle<AudioSource>,
}

/// Which music loop is playing, and the entity carrying it (so a state change can swap it).
#[derive(Resource)]
struct MusicState {
    combat: bool,
    entity: Entity,
}

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Sfx>()
            .insert_resource(GlobalVolume::new(Volume::Linear(MASTER_VOLUME)))
            .add_systems(Startup, load_audio)
            .add_systems(
                Update,
                (play_sfx, footsteps, crab_squitter, growl_stinger, update_music, mute_when_background),
            );
    }
}

/// Silence the whole mix whenever the game is in the **background** — its window unfocused (alt-tabbed,
/// another Space, minimised) or absent entirely (a headless run: the sim harness, a background/CI
/// session, or a devshot capture). Restores `MASTER_VOLUME` the instant the window regains focus. One
/// cheap resource write per frame; no per-sound bookkeeping. This is why running the game in the
/// background — as this project does for `devshot` screenshots — makes no noise.
fn mute_when_background(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut volume: ResMut<GlobalVolume>,
) {
    // No primary window at all (headless) ⇒ treat as background ⇒ muted.
    let focused = windows.iter().next().is_some_and(|w| w.focused);
    volume.volume = Volume::Linear(if focused { MASTER_VOLUME } else { 0.0 });
}

/// Load every handle, start the always-on wind bed, and start the calm music loop.
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
        growl: assets.load("audio/enemy/growl.ogg"),
        squitter: assets.load("audio/enemy/squitter.ogg"),
        footsteps: [
            assets.load("audio/foot/carpet_1.ogg"),
            assets.load("audio/foot/carpet_2.ogg"),
            assets.load("audio/foot/carpet_3.ogg"),
            assets.load("audio/foot/carpet_4.ogg"),
        ],
        wind: assets.load("audio/ambience/wind.ogg"),
        music_calm: assets.load("audio/music/calm.ogg"),
        music_combat: assets.load("audio/music/combat.ogg"),
    };

    // Backrooms wind bed — loops forever underneath everything.
    commands.spawn((AudioPlayer::new(a.wind.clone()), looped(WIND_VOL)));

    // Start in calm; `update_music` swaps to combat when an enemy enters LOS.
    let music = commands
        .spawn((AudioPlayer::new(a.music_calm.clone()), looped(MUSIC_VOL)))
        .id();
    commands.insert_resource(MusicState {
        combat: false,
        entity: music,
    });
    commands.insert_resource(a);
}

/// Play one clip per queued [`Sfx`], voice-capping the fire spam and jittering pitch so repeats
/// don't sound stamped.
fn play_sfx(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    mut msgs: MessageReader<Sfx>,
    mut rng: Local<u32>,
) {
    let mut fire_voices = 0usize;
    for sfx in msgs.read() {
        let (handle, vol, speed) = match sfx {
            Sfx::MoveOrder => (assets.move_order.clone(), UI_VOL, jitter(&mut rng, 0.05)),
            Sfx::Invalid => (assets.invalid.clone(), UI_VOL, jitter(&mut rng, 0.03)),
            Sfx::Fire => {
                // Cap concurrent muzzle blasts this frame; a whole squad firing stays a firefight.
                if fire_voices >= MAX_FIRE_VOICES {
                    continue;
                }
                fire_voices += 1;
                (assets.fire.clone(), 0.32, jitter(&mut rng, 0.15))
            }
            Sfx::ImpactWall => (assets.wall.clone(), 0.4, jitter(&mut rng, 0.12)),
            // Flesh hits, enemy bursts, and unit crunches are gory now (see `gore`).
            Sfx::ImpactFlesh => (assets.splat.clone(), 0.55, jitter(&mut rng, 0.12)),
            Sfx::EnemyDeath => (assets.squelch.clone(), 0.7, jitter(&mut rng, 0.08)),
            Sfx::UnitDeath => (assets.crunch.clone(), 0.85, jitter(&mut rng, 0.06)),
        };
        commands.spawn((AudioPlayer::new(handle), one_shot(vol, speed)));
        // A crunched unit layers a bone snap over the wet crunch.
        if matches!(sfx, Sfx::UnitDeath) {
            commands.spawn((
                AudioPlayer::new(assets.bone_snap.clone()),
                one_shot(0.7, jitter(&mut rng, 0.1)),
            ));
        }
    }
}

/// Squad footfalls from a single shared voice (never overlapping — that's what turned five units
/// into an army). Density scales linearly with the number of units actually walking, so a full
/// squad patters ~5× faster than a lone survivor and the sound audibly thins as members die. Kept
/// quiet so it's floor texture under the action.
fn footsteps(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    time: Res<Time>,
    units: Query<&Velocity, With<Unit>>,
    mut timer: Local<f32>,
    mut rng: Local<u32>,
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
        let idx = (next_u32(&mut rng) as usize) % assets.footsteps.len();
        commands.spawn((
            AudioPlayer::new(assets.footsteps[idx].clone()),
            one_shot(FOOT_VOL, jitter(&mut rng, 0.08)),
        ));
    }
}

/// Crab-swarm skitter from a single shared voice (never overlapping per-crab — the same fix as
/// `footsteps`). Density scales with the number of crabs currently near the squad, so a fresh
/// infestation chitters densely and the sound thins as the swarm is culled; silent when none are near.
fn crab_squitter(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    time: Res<Time>,
    crabs: Query<&Transform, With<Crab>>,
    units: Query<&Transform, (With<Unit>, Without<Crab>)>,
    mut timer: Local<f32>,
    mut rng: Local<u32>,
) {
    // Count crabs within SQUITTER_RANGE (planar) of any unit — the audible swarm.
    let near = crabs
        .iter()
        .filter(|c| {
            units
                .iter()
                .any(|u| (c.translation.xz() - u.translation.xz()).length() <= SQUITTER_RANGE)
        })
        .count();
    if near == 0 {
        *timer = SQUITTER_STRIDE; // armed, so the next crab that closes in skitters immediately
        return;
    }
    // More crabs ⇒ denser skittering, floored so it never machine-guns into mush (as with footsteps).
    let interval = (SQUITTER_STRIDE / near as f32).max(SQUITTER_MIN_STRIDE);
    *timer += time.delta_secs();
    if *timer >= interval {
        *timer = 0.0;
        commands.spawn((
            AudioPlayer::new(assets.squitter.clone()),
            one_shot(SQUITTER_VOL, jitter(&mut rng, 0.12)),
        ));
    }
}

/// Growl when a *visible* enemy first crosses inside [`GROWL_RANGE`] of any unit. Edge-triggered per
/// enemy so it stings once on sighting, not every frame, and re-arms when the enemy leaves range.
fn growl_stinger(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    dungeon: Res<Dungeon>,
    fog: Res<FogGrid>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
    units: Query<&Transform, (With<Unit>, Without<Enemy>)>,
    mut near: Local<HashMap<Entity, bool>>,
    mut rng: Local<u32>,
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
            commands.spawn((
                AudioPlayer::new(assets.growl.clone()),
                one_shot(GROWL_VOL, jitter(&mut rng, 0.1)),
            ));
        }
    }

    near.retain(|e, _| live.contains(e));
}

/// Swap the music loop when the squad's combat state flips. Combat = any enemy currently in fog LOS
/// (the same visibility the laser uses to decide it can shoot). Hard cut on transition; the wind bed
/// underneath keeps the seam from feeling empty.
fn update_music(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    dungeon: Res<Dungeon>,
    fog: Res<FogGrid>,
    // A cleared-but-standing Nest is `Hostile` (siege-killable) yet not a live threat, so exclude it —
    // otherwise the combat track latches on forever whenever an inert nest sits in the squad's LOS.
    enemies: Query<&Transform, (With<Hostile>, Without<crate::nest::Nest>)>,
    mut state: ResMut<MusicState>,
) {
    let combat = enemies
        .iter()
        .any(|tf| fog.visible_at(dungeon.world_to_cell(tf.translation)));
    if combat == state.combat {
        return;
    }
    state.combat = combat;
    commands.entity(state.entity).despawn();
    let handle = if combat {
        assets.music_combat.clone()
    } else {
        assets.music_calm.clone()
    };
    state.entity = commands
        .spawn((AudioPlayer::new(handle), looped(MUSIC_VOL)))
        .id();
}

/// One-shot playback: play once at `vol`/`speed`, then despawn the entity.
fn one_shot(vol: f32, speed: f32) -> PlaybackSettings {
    let mut s = PlaybackSettings::DESPAWN;
    s.volume = Volume::Linear(vol);
    s.speed = speed;
    s
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
