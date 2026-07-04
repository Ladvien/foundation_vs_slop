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

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::dungeon::Dungeon;
use crate::enemy::Enemy;
use crate::fog::FogGrid;
use crate::squad::{Unit, Velocity};

/// Looping-bed volumes and the mixing headroom for one-shots. Ambience and footsteps sit low so
/// the foreground (weapons, gore, growls) reads clearly over them.
const WIND_VOL: f32 = 0.22;
const MUSIC_VOL: f32 = 0.32;
const FOOT_VOL: f32 = 0.30;
/// UI / command blips (select, deselect, move order…). Deliberately way under the world sounds — a
/// faint tick you feel more than hear, so a fidgety player isn't machine-gunned with pings.
const UI_VOL: f32 = 0.12;

/// Seconds between footfalls with a *single* unit walking. The interval scales down linearly with
/// the number of movers (see `footsteps`), so a full squad patters ~5× faster and the sound
/// audibly thins as members die.
const STRIDE: f32 = 0.5;
/// Floor on the footfall interval, so even a full squad's patter never machine-guns.
const MIN_STRIDE: f32 = 0.12;
/// A unit is "walking" (and so contributes a footfall) once its planar speed clears this. Well
/// under the `UNIT_SPEED = 6.0` cruise, but above ORCA jitter so a settled blob stays quiet.
const FOOT_MIN_SPEED: f32 = 0.6;
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
    /// A single unit was selected (click or number key).
    Select,
    /// The whole squad was selected (key 6).
    SelectAll,
    /// Selection was cleared with Esc.
    Deselect,
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
    select: Handle<AudioSource>,
    select_all: Handle<AudioSource>,
    deselect: Handle<AudioSource>,
    move_order: Handle<AudioSource>,
    invalid: Handle<AudioSource>,
    fire: Handle<AudioSource>,
    wall: Handle<AudioSource>,
    flesh: Handle<AudioSource>,
    enemy_death: Handle<AudioSource>,
    growl: Handle<AudioSource>,
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
            .add_systems(Startup, load_audio)
            .add_systems(Update, (play_sfx, footsteps, growl_stinger, update_music));
    }
}

/// Load every handle, start the always-on wind bed, and start the calm music loop.
fn load_audio(mut commands: Commands, assets: Res<AssetServer>) {
    let a = AudioAssets {
        select: assets.load("audio/ui/select.ogg"),
        select_all: assets.load("audio/ui/select_all.ogg"),
        deselect: assets.load("audio/ui/deselect.ogg"),
        move_order: assets.load("audio/ui/move_order.ogg"),
        invalid: assets.load("audio/ui/invalid.ogg"),
        fire: assets.load("audio/weapon/fire.ogg"),
        wall: assets.load("audio/impact/wall.ogg"),
        flesh: assets.load("audio/impact/flesh.ogg"),
        enemy_death: assets.load("audio/impact/enemy_death.ogg"),
        growl: assets.load("audio/enemy/growl.ogg"),
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
            Sfx::Select => (assets.select.clone(), UI_VOL, jitter(&mut rng, 0.05)),
            Sfx::SelectAll => (assets.select_all.clone(), UI_VOL, jitter(&mut rng, 0.03)),
            Sfx::Deselect => (assets.deselect.clone(), UI_VOL, jitter(&mut rng, 0.05)),
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
            Sfx::ImpactFlesh => (assets.flesh.clone(), 0.55, jitter(&mut rng, 0.12)),
            Sfx::EnemyDeath => (assets.enemy_death.clone(), 0.7, jitter(&mut rng, 0.08)),
            // A unit going down reuses the heavy wall thud, pitched down so it reads as a body drop.
            Sfx::UnitDeath => (assets.wall.clone(), 0.7, 0.7 * jitter(&mut rng, 0.05)),
        };
        commands.spawn((AudioPlayer::new(handle), one_shot(vol, speed)));
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
    // machine-guns. ~0.5s/step for one survivor down to ~0.12s for a full five.
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
    enemies: Query<&Transform, With<Enemy>>,
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

/// Cheap LCG (Numerical Recipes constants), matching the project's hand-rolled RNG in `laser.rs` —
/// no RNG crate. Full-period from any seed, including the `Local<u32>` default of 0.
fn next_u32(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    *state
}

/// A float in [0, 1) from the LCG.
fn rand01(state: &mut u32) -> f32 {
    (next_u32(state) >> 8) as f32 / (1u32 << 24) as f32
}

/// A playback-speed multiplier of `1.0 ± amount`, so repeated one-shots don't sound identical.
fn jitter(state: &mut u32, amount: f32) -> f32 {
    1.0 + (rand01(state) * 2.0 - 1.0) * amount
}
