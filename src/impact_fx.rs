//! Laser-impact particle burst: a custom additive `Material` (see `assets/shaders/impact_fx.wgsl`)
//! played on a camera-facing quad wherever a bolt hits a wall. Every knob is loaded once at startup
//! from the `impact_fx:` slice of the unified `assets/config/config.ron`; there is no in-game panel.
//!
//! Decoupled trigger: anything that wants a burst pushes a world position into [`ImpactQueue`]
//! (the laser does this on wall hits); this plugin drains the queue and spawns the effect. So a
//! future enemy hit / explosion can reuse it by pushing to the same queue.

use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use serde::Deserialize;

/// World positions where an impact burst should play this frame (drained each frame).
#[derive(Resource, Default)]
pub struct ImpactQueue(pub Vec<Vec3>);

/// GPU uniform — mirrors the `ImpactSettings` struct in `impact_fx.wgsl` (field order + types).
#[derive(Clone, ShaderType)]
struct ImpactUniform {
    color_a: Vec4,
    color_b: Vec4,
    intensity: f32,
    spread: f32,
    speed: f32,
    particle_size: f32,
    gravity: f32,
    spawn_time: f32,
    duration: f32,
    seed: f32,
    particle_count: i32,
}

/// The custom additive impact material.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct ImpactMaterial {
    #[uniform(0)]
    settings: ImpactUniform,
}

impl Material for ImpactMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/impact_fx.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        // Blend with a brightness-driven alpha: dark areas are transparent, so the square quad
        // disappears and only the glowing burst composites over the scene.
        AlphaMode::Blend
    }
}

/// The impact-burst knobs, deserialized once at startup from the `impact_fx:` slice of the unified
/// `assets/config/config.ron` (read-only — there is no in-game panel and nothing serializes these back
/// out). A field of `crate::config::GameConfig`.
#[derive(Resource, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ImpactFxSettings {
    particle_count: i32,
    color_a: [f32; 3],
    color_b: [f32; 3],
    intensity: f32,
    spread: f32,
    speed: f32,
    particle_size: f32,
    gravity: f32,
    duration: f32,
    quad_size: f32,
}

impl Default for ImpactFxSettings {
    fn default() -> Self {
        ImpactFxSettings {
            particle_count: 24,
            color_a: [1.0, 0.5, 0.15],
            color_b: [1.0, 0.85, 0.4],
            intensity: 1.0,
            spread: 0.6,
            speed: 1.0,
            particle_size: 50.0,
            gravity: 0.3,
            duration: 0.6,
            quad_size: 3.0,
        }
    }
}

impl ImpactFxSettings {
    fn to_uniform(&self, spawn_time: f32, seed: f32) -> ImpactUniform {
        ImpactUniform {
            color_a: Vec4::new(self.color_a[0], self.color_a[1], self.color_a[2], 1.0),
            color_b: Vec4::new(self.color_b[0], self.color_b[1], self.color_b[2], 1.0),
            intensity: self.intensity,
            spread: self.spread,
            speed: self.speed,
            particle_size: self.particle_size,
            gravity: self.gravity,
            spawn_time,
            duration: self.duration,
            seed,
            particle_count: self.particle_count,
        }
    }
}

/// A live burst entity; despawns when the clock passes `despawn_at`.
#[derive(Component)]
struct ImpactFx {
    despawn_at: f32,
}

/// Shared quad mesh for every burst.
#[derive(Resource)]
struct ImpactAssets {
    quad: Handle<Mesh>,
}

pub struct ImpactFxPlugin;

impl Plugin for ImpactFxPlugin {
    fn build(&self, app: &mut App) {
        // Required config — one path, no fallback. The `impact_fx:` slice comes from the unified
        // `assets/config/config.ron`, loaded + validated once by `ConfigPlugin` (registered first).
        let settings = app.world().resource::<crate::config::GameConfig>().impact_fx.clone();
        app.add_plugins(MaterialPlugin::<ImpactMaterial>::default())
            .init_resource::<ImpactQueue>()
            .insert_resource(settings)
            .add_systems(Startup, setup_impact_assets)
            .add_systems(Update, (drain_impacts, despawn_impacts));
    }
}

fn setup_impact_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(ImpactAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
    });
}

/// Spawn a burst for each queued impact position, oriented to face the (fixed) iso camera.
#[allow(clippy::too_many_arguments)]
fn drain_impacts(
    mut commands: Commands,
    time: Res<Time>,
    mut queue: ResMut<ImpactQueue>,
    settings: Res<ImpactFxSettings>,
    assets: Res<ImpactAssets>,
    mut materials: ResMut<Assets<ImpactMaterial>>,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    mut seed: Local<u32>,
) {
    if queue.0.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    let cam_rot = camera.rotation();
    let cam_pos = camera.translation();
    for pos in queue.0.drain(..) {
        *seed = seed.wrapping_add(1);
        // Nudge the quad toward the camera so the additive burst isn't clipped by the wall it hit.
        let toward = (cam_pos - pos).normalize_or_zero();
        let material = materials.add(ImpactMaterial {
            settings: settings.to_uniform(now, *seed as f32 * 0.618),
        });
        commands.spawn((
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(pos + toward * 0.4)
                .with_rotation(cam_rot)
                .with_scale(Vec3::splat(settings.quad_size)),
            ImpactFx {
                despawn_at: now + settings.duration,
            },
        ));
    }
}

fn despawn_impacts(mut commands: Commands, time: Res<Time>, bursts: Query<(Entity, &ImpactFx)>) {
    let now = time.elapsed_secs();
    for (entity, fx) in &bursts {
        if now >= fx.despawn_at {
            commands.entity(entity).despawn();
        }
    }
}
