//! The mold's senses — a small CPU-written control texture (one texel per dungeon cell) that the compute
//! chain reads to steer on world state. This is the ONLY channel by which the world reaches the mold, and
//! it is strictly one-way: nothing here reads mold state back into gameplay (see the `mod.rs` firewall).
//!
//! # Channels (`Rgba8Unorm`)
//! | Ch | Meaning | Source |
//! |----|---------|--------|
//! | `R` | chemoattractant | blood pools + nests — the mold forages toward carnage and hoards |
//! | `G` | light / gaze repellent | cells a squad unit currently sees, attenuated by habituation |
//! | `B` | disturbance repellent | squad unit proximity — footsteps scatter the mold |
//! | `A` | substrate mask | `0` = void · `0.5` = floor, never seen · `1` = floor, explored |
//!
//! # Why `A` is three-state, not a bool
//! The mold *grows* on any floor, seen or not — a room you have never entered is exactly where it should be
//! ripest. But it must not be *drawn* on floor the player has never explored, or the coating would trace
//! the corridor layout straight through the fog of war and leak the map. So growth keys off "is floor"
//! (`A >= 0.25`) while rendering keys off "is explored" (`A >= 0.75`). One channel, two thresholds.
//!
//! # Habituation
//! `G` is not simply "is this cell watched". Each watched cell accumulates habituation and its repellent
//! fades; once the gaze leaves, habituation decays and the cell becomes frightening again. This gives the
//! mold a memory: a corridor the squad keeps staring down stops scaring it, and re-scares it after they
//! leave. Grounded in Boisseau, Vogel & Dussutour (2016), 10.1098/rspb.2016.0446 — *P. polycephalum*
//! habituates to a repeatedly-presented harmless repellent and shows spontaneous recovery when it is
//! withheld.

use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;

use crate::dungeon::Dungeon;
use crate::fog::FogGrid;
use crate::gore::BloodPool;
use crate::nest::Nest;
use crate::squad::Unit;

use super::{field, MyceliaConfig, CONTROL_SIZE};

/// Chemoattractant splat radius (cells) around a nest's floor position.
const NEST_RADIUS_CELLS: f32 = 2.5;
/// Disturbance splat radius (cells) around each squad unit.
const UNIT_RADIUS_CELLS: f32 = 2.0;
/// Floor on a blood pool's splat radius (cells), so even a small stain is smelled.
const BLOOD_MIN_RADIUS_CELLS: f32 = 1.0;

/// The control texture handle, extracted so the render world can bind it.
#[derive(Resource, Clone, ExtractResource)]
pub struct MoldControlImage(pub Handle<Image>);

/// Main-world scratch for the control texture: the CPU-side pixel buffer we rewrite each `Update`, plus
/// the per-cell habituation accumulator (which is *state*, so it must persist between frames).
#[derive(Resource)]
pub struct MoldControl {
    image: Handle<Image>,
    cpu: Vec<u8>,
    habituation: Vec<f32>,
}

impl MoldControl {
    fn cells() -> usize {
        (CONTROL_SIZE * CONTROL_SIZE) as usize
    }
}

/// Create the control texture and its CPU mirrors. Runs once at `Startup`.
pub(super) fn setup_control(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = images.add(field::control_texture(CONTROL_SIZE));
    commands.insert_resource(MoldControlImage(image.clone()));
    commands.insert_resource(MoldControl {
        image,
        cpu: vec![0u8; MoldControl::cells() * field::CONTROL_BYTES_PER_TEXEL],
        habituation: vec![0.0; MoldControl::cells()],
    });
}

/// Rasterize world state into the control texture, once per `Update`.
///
/// Cosmetic and read-only with respect to gameplay: it queries `Transform`s and the fog/dungeon grids and
/// mutates nothing but its own buffers. Uses `Time<Real>` to match the rest of the mold (which keeps
/// breathing while the game is paused).
pub(super) fn write_control(
    mut control: ResMut<MoldControl>,
    cfg: Res<MyceliaConfig>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time<Real>>,
    dungeon: Option<Res<Dungeon>>,
    fog: Option<Res<FogGrid>>,
    pools: Query<&Transform, With<BloodPool>>,
    nests: Query<&Nest>,
    units: Query<&Transform, With<Unit>>,
) {
    // The dungeon is the substrate; without it there is nothing to coat. (Menu//loading frames.)
    let Some(dungeon) = dungeon else {
        return;
    };
    let dt = time.delta_secs();
    let size = CONTROL_SIZE as i32;

    // Split the borrow so we can read `cpu` while mutating `habituation` (and vice versa).
    let MoldControl { image, cpu, habituation } = &mut *control;

    // ── Pass 1: per-cell fog (light + habituation) and the walkable mask ──────────────────────────────
    for y in 0..size {
        for x in 0..size {
            let cell = IVec2::new(x, y);
            let i = (y * size + x) as usize;

            let watched = fog.as_ref().is_some_and(|f| f.visible_at(cell));
            let hab = &mut habituation[i];
            if watched {
                *hab = (*hab + cfg.hab_rate * dt).min(1.0);
            } else {
                *hab = (*hab - cfg.hab_recover * dt).max(0.0);
            }

            // Only a *currently seen* cell repels. Explored-but-dark and never-seen cells are equally safe
            // — which is exactly the ambience we want: the mold blooms wherever nobody is looking.
            let light = if watched { 1.0 - *hab * cfg.hab_strength } else { 0.0 };

            // A: three-state substrate mask. Agents grow on any floor; only *explored* floor is drawn.
            let substrate = if !dungeon.is_floor(cell) {
                0
            } else if fog.as_ref().is_none_or(|f| f.seen_at(cell)) {
                255
            } else {
                128
            };

            let base = i * field::CONTROL_BYTES_PER_TEXEL;
            cpu[base] = 0; // R: chemo — accumulated in pass 2
            cpu[base + 1] = to_u8(light); // G: light/gaze
            cpu[base + 2] = 0; // B: disturbance — accumulated in pass 2
            cpu[base + 3] = substrate; // A: 0 void / 128 unseen floor / 255 explored floor
        }
    }

    // ── Pass 2: splat the point sources ───────────────────────────────────────────────────────────────
    // Blood pools and nests attract (R); squad units disturb (B).
    for t in &pools {
        // A pool's footprint lives only in its Transform scale (there is no radius field); the quad spans
        // [-1,1] in local space, so the half-extent in world units is `scale * 0.5`.
        let radius = (t.scale.x * 0.5).max(BLOOD_MIN_RADIUS_CELLS);
        splat(cpu, dungeon.world_to_cell(t.translation), radius, 0);
    }
    for nest in &nests {
        splat(cpu, dungeon.world_to_cell(nest.pos), NEST_RADIUS_CELLS, 0);
    }
    for t in &units {
        splat(cpu, dungeon.world_to_cell(t.translation), UNIT_RADIUS_CELLS, 2);
    }

    // ── Upload ────────────────────────────────────────────────────────────────────────────────────────
    // Mutating through `Assets<Image>` marks the asset changed, so Bevy re-uploads it to the GPU.
    // `get_mut` hands back an `AssetMut` guard; touching it through `DerefMut` is what flags the asset as
    // changed, which is precisely what triggers Bevy's re-upload of the texture to the GPU.
    let Some(mut gpu_image) = images.get_mut(&*image) else {
        return;
    };
    match gpu_image.data.as_mut() {
        Some(data) if data.len() == cpu.len() => data.copy_from_slice(cpu),
        _ => gpu_image.data = Some(cpu.clone()),
    }
}

/// Additively splat a radial falloff into one channel of the control buffer, saturating at 255.
fn splat(cpu: &mut [u8], center: IVec2, radius_cells: f32, channel: usize) {
    let size = CONTROL_SIZE as i32;
    let r = radius_cells.max(0.5);
    let ri = r.ceil() as i32;
    for dy in -ri..=ri {
        for dx in -ri..=ri {
            let cell = center + IVec2::new(dx, dy);
            if cell.x < 0 || cell.y < 0 || cell.x >= size || cell.y >= size {
                continue;
            }
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            if dist > r {
                continue;
            }
            // Smooth falloff to the rim so the gradient is sensible to steer on.
            let strength = 1.0 - (dist / r);
            let idx = (cell.y * size + cell.x) as usize * field::CONTROL_BYTES_PER_TEXEL + channel;
            cpu[idx] = cpu[idx].saturating_add(to_u8(strength));
        }
    }
}

/// Map a `0..=1` strength to a `Rgba8Unorm` byte, clamping out-of-range inputs.
fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}
