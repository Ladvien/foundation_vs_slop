//! **Blood lens** — blood spattered on the "camera lens," flashed on a kill and fading out. A cheap,
//! high-impact impact cue (gore codex's screen-space layer), done as the codex's recommended form: a
//! full-screen UI overlay of transparent blood PNGs.
//!
//! Each kill spawns a *fresh burst* of several small blood decals at **random** screen positions,
//! sizes, texture sub-regions, and flips — so no two splashes look alike and there's no fixed
//! pattern. Each decal fades out over ~1 s and despawns. (Deliberately not a second
//! `FullscreenMaterial`: two of those collide on Bevy 0.19's shared post-process bind-group layout.)

use bevy::prelude::*;

/// Pending splash bursts to render (one per kill). Spiked by `gore`, drained by [`spawn_lens_splats`].
#[derive(Resource, Default)]
pub struct BloodLens {
    pending: u32,
}

impl BloodLens {
    /// Request a lens splash. `amount` is ignored (each kill = one burst); kept for the call site.
    pub fn splash(&mut self, _amount: f32) {
        self.pending = (self.pending + 1).min(3);
    }
}

/// Handle to the blood decal texture (an atlas of splatters with alpha), loaded once.
#[derive(Resource)]
struct LensImage(Handle<Image>);

/// One on-screen blood decal, fading out.
#[derive(Component)]
struct LensSplat {
    spawn_time: f32,
    life: f32,
}

/// Source atlas size (px) — `blood_base`/`blood_lens` are 1024².
const ATLAS: f32 = 1024.0;
/// Peak decal alpha (a splatter, not an opaque wall — the scene stays readable).
const SPLAT_ALPHA: f32 = 0.82;
/// Decals per kill.
const SPLATS_PER_BURST: u32 = 6;

pub struct BloodLensPlugin;

impl Plugin for BloodLensPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BloodLens>()
            .add_systems(Startup, load_image)
            .add_systems(Update, (spawn_lens_splats, fade_lens_splats));
    }
}

fn load_image(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(LensImage(assets.load("textures/blood/blood_lens.png")));
}

/// Deterministic hash → f32 in [0,1) (PCG-style), matching `gore`'s texture-free noise philosophy.
fn hash_f32(x: u32) -> f32 {
    let mut h = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    h = ((h >> ((h >> 28).wrapping_add(4))) ^ h).wrapping_mul(277_803_737);
    h = (h >> 22) ^ h;
    (h as f32) / (u32::MAX as f32)
}

/// Drain pending bursts: each spawns a spray of randomized blood decals across the screen.
fn spawn_lens_splats(
    mut commands: Commands,
    time: Res<Time>,
    image: Res<LensImage>,
    mut lens: ResMut<BloodLens>,
    mut seed: Local<u32>,
) {
    if lens.pending == 0 {
        return;
    }
    let bursts = lens.pending;
    lens.pending = 0;
    let now = time.elapsed_secs();

    for _ in 0..bursts {
        for _ in 0..SPLATS_PER_BURST {
            *seed = seed.wrapping_add(1);
            let b = seed.wrapping_mul(2_654_435_761);
            let ha = hash_f32(b.wrapping_add(1));
            let hb = hash_f32(b.wrapping_add(2));
            let hc = hash_f32(b.wrapping_add(3));
            let hd = hash_f32(b.wrapping_add(4));
            let he = hash_f32(b.wrapping_add(5));
            let hf = hash_f32(b.wrapping_add(6));
            let hg = hash_f32(b.wrapping_add(7));
            let hh = hash_f32(b.wrapping_add(8));

            // Random square sub-region of the atlas → a random splatter shape.
            let rs = 200.0 + 300.0 * hb;
            let rx = ha * (ATLAS - rs);
            let ry = hc * (ATLAS - rs);
            // Random on-screen size + position (allowed a little off-edge so some hug the edges).
            let display = 110.0 + 300.0 * hd;
            let left = -10.0 + 100.0 * he;
            let top = -10.0 + 100.0 * hf;

            commands.spawn((
                LensSplat {
                    spawn_time: now,
                    life: 0.85 + 0.6 * hg,
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(left),
                    top: Val::Percent(top),
                    width: Val::Px(display),
                    height: Val::Px(display),
                    ..default()
                },
                ImageNode {
                    image: image.0.clone(),
                    color: Color::srgba(1.0, 0.85, 0.85, SPLAT_ALPHA),
                    rect: Some(Rect::new(rx, ry, rx + rs, ry + rs)),
                    // Random flips stand in for rotation, so repeats of the same sub-region differ.
                    flip_x: hg > 0.5,
                    flip_y: hh > 0.5,
                    ..default()
                },
                GlobalZIndex(50),
                Pickable::IGNORE,
            ));
        }
    }
}

/// Fade each decal out over its life and despawn it when done.
fn fade_lens_splats(
    mut commands: Commands,
    time: Res<Time>,
    mut splats: Query<(Entity, &LensSplat, &mut ImageNode)>,
) {
    let now = time.elapsed_secs();
    for (entity, splat, mut img) in &mut splats {
        let age = (now - splat.spawn_time) / splat.life.max(0.001);
        if age >= 1.0 {
            commands.entity(entity).despawn();
        } else {
            img.color = img.color.with_alpha(SPLAT_ALPHA * (1.0 - age));
        }
    }
}
