//! Shared health + floating health bars.
//!
//! `Health` is a single component worn by both squad units and enemies, so one pair of systems can
//! render a bar over anything with hit points. Bars are **camera-facing quads** (the project's
//! established billboard recipe — see `impact_fx.rs` / `enemy.rs`), each carrying a tiny
//! [`HealthBarMaterial`] whose `fraction` uniform drives fill width and color
//! (`assets/shaders/health_bar.wgsl`). Legible health feedback keeps a fight readable and tunable,
//! an adaptive-difficulty affordance (McKay et al., "Implementing Adaptive Game Difficulty Balancing
//! in Serious Games", IEEE Trans. Games 2018, DOI 10.1109/tg.2018.2791019).

use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

/// Hit points for any combatant (units and enemies alike). `current` is clamped-consumed by damage;
/// `max` is fixed at spawn and used for the bar fraction.
#[derive(Component)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Health {
    pub fn new(max: f32) -> Self {
        Health { current: max, max }
    }

    /// Remaining health in [0, 1].
    pub fn fraction(&self) -> f32 {
        if self.max <= 0.0 {
            0.0
        } else {
            (self.current / self.max).clamp(0.0, 1.0)
        }
    }
}

/// Height of the bar above the owner's transform origin (owners sit near Y=0; head ≈ 1.0).
const BAR_Y: f32 = 1.5;
/// Bar quad size in world units (wide and short).
const BAR_WIDTH: f32 = 1.1;
const BAR_HEIGHT: f32 = 0.16;

/// A bar entity's link back to the combatant it displays.
#[derive(Component)]
struct HealthBar {
    owner: Entity,
}

/// Marks an owner that already has a bar, so `attach_health_bars` runs once per combatant.
#[derive(Component)]
struct HasHealthBar;

/// GPU uniform — mirrors `HealthBarSettings` in `health_bar.wgsl` (field order + types).
#[derive(Clone, ShaderType)]
struct HealthBarUniform {
    fraction: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

/// The custom health-bar material.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct HealthBarMaterial {
    #[uniform(0)]
    settings: HealthBarUniform,
}

impl Material for HealthBarMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/health_bar.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
}

/// Shared quad mesh for every bar.
#[derive(Resource)]
struct HealthBarAssets {
    quad: Handle<Mesh>,
}

pub struct HealthPlugin;

impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<HealthBarMaterial>::default())
            .add_systems(Startup, setup_health_bar_assets)
            .add_systems(Update, (attach_health_bars, update_health_bars).chain());
    }
}

fn setup_health_bar_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(HealthBarAssets {
        quad: meshes.add(Rectangle::new(BAR_WIDTH, BAR_HEIGHT)),
    });
}

/// Give every combatant that doesn't have one a floating bar entity (top-level, not a child — so the
/// figurine's non-unit scale doesn't distort it).
fn attach_health_bars(
    mut commands: Commands,
    assets: Res<HealthBarAssets>,
    mut materials: ResMut<Assets<HealthBarMaterial>>,
    owners: Query<(Entity, &Health), Without<HasHealthBar>>,
) {
    for (owner, health) in &owners {
        let material = materials.add(HealthBarMaterial {
            settings: HealthBarUniform {
                fraction: health.fraction(),
                _pad0: 0.0,
                _pad1: 0.0,
                _pad2: 0.0,
            },
        });
        commands.spawn((
            HealthBar { owner },
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(material),
            Transform::default(),
        ));
        commands.entity(owner).insert(HasHealthBar);
    }
}

/// Track each bar to its owner: reposition above the head, face the camera, refresh the fill, and
/// mirror the owner's visibility (so a fog-hidden enemy's bar hides too). Orphaned bars despawn.
fn update_health_bars(
    mut commands: Commands,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    owners: Query<(&Transform, &Health, &Visibility), Without<HealthBar>>,
    mut bars: Query<
        (
            Entity,
            &HealthBar,
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<HealthBarMaterial>,
        ),
        Without<Health>,
    >,
    mut materials: ResMut<Assets<HealthBarMaterial>>,
) {
    let cam_rot = camera.rotation();
    for (bar_entity, bar, mut tf, mut vis, mat_handle) in &mut bars {
        let Ok((owner_tf, health, owner_vis)) = owners.get(bar.owner) else {
            // Owner is gone — clean up its bar.
            commands.entity(bar_entity).despawn();
            continue;
        };
        tf.translation = owner_tf.translation + Vec3::Y * BAR_Y;
        tf.rotation = cam_rot;
        *vis = *owner_vis;
        if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            mat.settings.fraction = health.fraction();
        }
    }
}
