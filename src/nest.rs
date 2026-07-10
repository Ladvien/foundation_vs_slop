//! Dimensional nest portal — the crabs' home. A pulsating half-sphere dome (custom shader,
//! `assets/shaders/nest.wgsl`) that crabs haul scavenged meat into; a full hoard births new crabs
//! (see `crab::nest_reproduce`). The dome is a true **hemisphere** ([`nest_dome_mesh`]): its flat rim
//! seats flush on a wall's room-side face and it bulges into the room like a pimple. Because no geometry
//! sits behind that face, the camera-side wall cutaway (the dungeon's knee-wall squash) can never reveal a "back"
//! poking through the thin (`WALL_THICKNESS`) slab — the failure mode of the earlier squashed full sphere.
//!
//! A nest is a valid squad target: it carries `Hostile` + `Health`, so lasers can destroy it (killing
//! its birth loop). Each nest also owns a prebuilt `FlowField` toward its floor delivery cell, so a
//! carried chunk paths to it over walkable floor instead of beelining through walls.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::dungeon::Dungeon;
use crate::enemy::Hostile;
use crate::flowfield::FlowField;
use crate::health::Health;

/// Radius of the hemisphere rim across the wall face (its tangent axes). Sized so the rim
/// (`NEST_WALL_HEIGHT ± NEST_RADIUS`) stays within the wall and cell face.
const NEST_RADIUS: f32 = 0.4;
/// Depth the dome protrudes along the wall's normal into the room (the pimple's height). A shade under
/// the radius → a rounded, slightly-shallow bulge. Nothing sits behind the wall face (hemisphere).
const NEST_DEPTH: f32 = 0.38;
/// Height up the wall to seat the dome centre (its flat rim's centre on the face). Mid-wall, so it
/// tracks the ceiling height rather than a hardcoded value. Nests are seated only on full-height walls
/// (the crab placement pass skips the camera-facing knee walls — see `crab.rs`), so mid-`WALL_HEIGHT`
/// always lands on solid wall and the dome reads as seated, never floating above a short E/S wall.
const NEST_WALL_HEIGHT: f32 = crate::dungeon::WALL_HEIGHT * 0.5;
/// Nest hit points. Sized so a focused squad razes it in a few seconds at the current nerfed
/// `LASER_DAMAGE` (see `laser.rs`); raise alongside laser power for a longer siege.
const NEST_HP: f32 = 60.0;

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
    /// Meat delivered so far — drives the portal glow (visual feedback; see `update_nests`).
    pub hoard: f32,
    /// Floor delivery position (y=0) — the haul destination + birth site, and the `flow` goal.
    pub pos: Vec3,
    /// Prebuilt flow field toward `pos` over walkable floor, so hauled chunks route around walls. Static
    /// (nests don't move), so it's built once at spawn and never rebuilt. `Arc` for cheap cloning/sharing.
    pub flow: Arc<FlowField>,
    /// Countdown to the next timed crab respawn (see `crab::nest_reproduce`).
    pub respawn_timer: f32,
    /// Feeding surge: a decaying accumulator raised by the weight of each delivered meat chunk. It
    /// shortens the respawn interval by up to ~10× while the nest is fed, then fades — so a well-fed
    /// nest births crabs far faster (see `crab::nest_reproduce` and the DELIVER path in `carry_gibs`).
    pub spawn_boost: f32,
}

/// Spawn one nest portal bulging from a wall. `wall_point` is a point on the wall's inner face (y=0),
/// `inward_normal` points into the room, `delivery_pos` is the floor cell in front where crabs drop meat.
/// The dome is seated part-buried in the wall at mid-height. Returns the entity so a cluster can be
/// associated with its nest. Fails loudly (returns `None`) if the delivery cell isn't floor — no fallback.
pub fn spawn_nest(
    commands: &mut Commands,
    materials: &mut Assets<NestMaterial>,
    dome: Handle<Mesh>,
    wall_point: Vec3,
    inward_normal: Vec3,
    delivery_pos: Vec3,
    dungeon: &Dungeon,
) -> Option<Entity> {
    let goal = dungeon.world_to_cell(delivery_pos);
    let flow = FlowField::build(dungeon, goal)?;

    let material = materials.add(NestMaterial {
        settings: NestUniform {
            hoard: 0.0,
            radius: NEST_RADIUS,
        },
    });
    // Seat the hemisphere as a pimple: rotate its dome axis (local +Y) onto `inward_normal` so the flat
    // rim lies in the wall's room-side face and the dome bulges into the room. Local scale is the rim
    // radius on the tangent axes (X/Z) and the protrusion depth on the dome axis (Y, pre-rotation). A
    // 1 cm nudge into the room keeps the rim off the wall face so it can't z-fight it. `inward_normal`
    // is axis-aligned (±X/±Z), never ±Y, so the arc rotation is well-defined. Nothing sits behind the
    // face, so the camera-side wall cutaway can't reveal a back poking through the slab.
    let rotation = Quat::from_rotation_arc(Vec3::Y, inward_normal);
    let scale = Vec3::new(NEST_RADIUS, NEST_DEPTH, NEST_RADIUS);
    let center = wall_point.with_y(NEST_WALL_HEIGHT) + inward_normal * 0.01;
    let id = commands
        .spawn((
            Nest {
                hoard: 0.0,
                pos: delivery_pos.with_y(0.0),
                flow: Arc::new(flow),
                respawn_timer: 0.0,
                spawn_boost: 0.0,
            },
            // Squad-killable: Hostile makes it a laser target, Health gives it durability + a floating
            // health bar so the player can see the siege progress. The CPU laser hit-volume (a sphere the
            // size of the dome rim) is paired with the dome mesh so bolts test it headlessly.
            Hostile,
            Health::new(NEST_HP),
            (
                Mesh3d(dome),
                crate::laser::LaserTarget { radius: NEST_RADIUS, half_height: 0.0 },
            ),
            MeshMaterial3d(material),
            Transform::from_translation(center)
                .with_rotation(rotation)
                .with_scale(scale),
        ))
        .id();
    Some(id)
}

/// A unit hemisphere dome — a cap of radius 1 bulging along +Y with its flat rim on the y=0 plane (no
/// base disc: the rim seats flush on a wall, so the open side is never seen). UVs are a disc projection
/// (pole → uv centre, rim → unit circle) so `nest.wgsl`'s radial swirl sits centred on the dome.
/// Single-sided outward winding (LearnOpenGL sphere convention; wgpu default CCW-front + back-cull), so
/// there is no "back" for the camera-side wall cutaway to reveal — the fix for the portal poking through
/// walls. Built from scratch (Bevy has no hemisphere primitive).
pub fn nest_dome_mesh() -> Mesh {
    const RINGS: usize = 16; // pole → rim
    const SECTORS: usize = 24; // around the dome
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    for i in 0..=RINGS {
        // phi: 0 at the pole (top of dome) → π/2 at the rim (on the y=0 wall plane).
        let phi = std::f32::consts::FRAC_PI_2 * i as f32 / RINGS as f32;
        let (sp, cp) = phi.sin_cos();
        for j in 0..=SECTORS {
            let theta = std::f32::consts::TAU * j as f32 / SECTORS as f32;
            let (st, ct) = theta.sin_cos();
            let (x, y, z) = (sp * ct, cp, sp * st);
            positions.push([x, y, z]);
            normals.push([x, y, z]); // unit sphere → the position IS the outward normal
            uvs.push([0.5 + 0.5 * x, 0.5 + 0.5 * z]); // disc projection: pole→centre, rim→unit circle
        }
    }
    let stride = (SECTORS + 1) as u32;
    let mut idx: Vec<u32> = Vec::new();
    for i in 0..RINGS as u32 {
        for j in 0..SECTORS as u32 {
            let k1 = i * stride + j;
            let k2 = k1 + stride;
            idx.extend_from_slice(&[k1, k1 + 1, k2, k1 + 1, k2 + 1, k2]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

pub struct NestPlugin;

impl Plugin for NestPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<NestMaterial>::default())
            // Deposit the nest-defense alarm before the stig deposits drain, so the bloom is live this
            // frame (mirrors `crab::crab_alarm_on_damage` ordering).
            // Pinned sim (alarm deposit ordered before the stig drain, death) on `FixedUpdate`; the
            // material/visual refresh stays on `Update`.
            .add_systems(
                FixedUpdate,
                (nest_alarm.before(crate::ai::AiSet::Deposits), despawn_dead_nests),
            )
            .add_systems(Update, update_nests);
    }
}

/// Flood the LOCAL ALARM pheromone around a nest whenever it takes damage (its `Health` changes below
/// full), so nearby crabs muster to defend it (stop fleeing, converge on the squad) — the same
/// `FieldId::ALARM` channel a wounded crab uses (`crab::crab_alarm_on_damage`), just a stronger, wider
/// deposit. This replaces the old GLOBAL `NestAlarm` berserk flag, which turned every crab mapwide
/// fearless on any nest hit; the deposit is spatially scoped, so poking one nest only rouses its own
/// swarm. The field's evaporation is the automatic call-off (no explicit timer). Change-detection means a
/// hit this frame re-floods it. A stigmergic colony-defense recruitment (Heylighen, CSR 2016).
fn nest_alarm(
    nests: Query<(Ref<Health>, &Transform), With<Nest>>,
    mut deposits: ResMut<crate::ai::field::StigDeposits>,
    sim: Res<crate::sim::SimTuning>,
) {
    for (hp, tf) in &nests {
        if hp.is_changed() && !hp.is_added() && hp.current < hp.max {
            deposits.0.push(crate::ai::field::Deposit {
                pos: tf.translation,
                field: crate::ai::field::FieldId::ALARM,
                amount: sim.deposit.alarm_nest,
            });
        }
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

/// Razed by the squad: a nest whose Health hit zero despawns, ending its birth loop (it drops out of
/// `crab::nest_reproduce`'s query) and orphaning any in-flight haul (which then aborts and drops).
fn despawn_dead_nests(mut commands: Commands, nests: Query<(Entity, &Health), With<Nest>>) {
    for (e, hp) in &nests {
        if hp.current <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}
