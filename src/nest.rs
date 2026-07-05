//! Dimensional nest portal — the crabs' home. A pulsating half-sphere dome (custom shader,
//! `assets/shaders/nest.wgsl`) that crabs haul scavenged meat into; a full hoard births new crabs
//! (see `crab::nest_reproduce`). The dome is a full `Sphere` centred on the floor (`y=0`) — its lower
//! half is below the floor and occluded, so the top reads as a half-sphere for free.

use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

/// World radius of a nest dome.
const NEST_RADIUS: f32 = 0.9;

/// GPU uniform — must byte-match `NestSettings` in `nest.wgsl`.
#[derive(Clone, ShaderType)]
struct NestUniform {
    hoard: f32,
    radius: f32,
}

/// The portal's custom fullscreen-fractal material.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct NestMaterial {
    #[uniform(0)]
    settings: NestUniform,
}

impl Material for NestMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/nest.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
    }
}

/// A dimensional nest: the crabs' delivery + birth anchor. `hoard` is the meat delivered so far.
#[derive(Component)]
pub struct Nest {
    pub hoard: f32,
    /// Floor position (carry destination + birth site) — read by the carry/reproduction systems.
    #[allow(dead_code)]
    pub pos: Vec3,
}

/// Spawn one nest portal at `pos` (centre at floor y=0 → a dome). `dome` is a unit `Sphere` mesh the
/// caller shares across nests. Returns the entity so a cluster can be associated with its nest.
pub fn spawn_nest(
    commands: &mut Commands,
    materials: &mut Assets<NestMaterial>,
    dome: Handle<Mesh>,
    pos: Vec3,
) -> Entity {
    let material = materials.add(NestMaterial {
        settings: NestUniform {
            hoard: 0.0,
            radius: NEST_RADIUS,
        },
    });
    commands
        .spawn((
            Nest {
                hoard: 0.0,
                pos: pos.with_y(0.0),
            },
            Mesh3d(dome),
            MeshMaterial3d(material),
            Transform::from_translation(pos.with_y(0.0)).with_scale(Vec3::splat(NEST_RADIUS)),
        ))
        .id()
}

pub struct NestPlugin;

impl Plugin for NestPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<NestMaterial>::default())
            .add_systems(Update, update_nests);
    }
}

/// Push each nest's live hoard into its material uniform so the portal brightens as it fills.
fn update_nests(
    nests: Query<(&Nest, &MeshMaterial3d<NestMaterial>)>,
    mut materials: ResMut<Assets<NestMaterial>>,
) {
    for (nest, handle) in &nests {
        if let Some(mut mat) = materials.get_mut(&handle.0) {
            mat.settings.hoard = nest.hoard;
        }
    }
}
