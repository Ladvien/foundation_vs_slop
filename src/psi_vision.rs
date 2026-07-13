//! **Psionic field-sight** — the squad's Psionic can literally see the danger fields.
//!
//! Fear in this game is stigmergic: a unit's FEAR drive tracks the local value of the threat channels its
//! enemies emit (`ai::faction`). Those channels are real, spatial, and already simulated — crabs lay
//! `THREAT_CRAB` wherever they crowd, the watcher radiates `THREAT_ANOMALY` through walls. Everyone else
//! only *feels* them. The Psionic sees them, drawn as a heat wash across the floor.
//!
//! This is why the role's perception is unlike the others': `PSI_SIGHT` reads the anomaly through walls
//! rather than through line of sight (`squad_ai::perception`). The overlay is the visible face of a sense
//! the simulation already had.
//!
//! Diegetic rather than a HUD readout, on purpose — spatial UI in Fagerholt & Lorentzon's taxonomy;
//! removing non-diegetic screen elements measurably raises involvement (Kennedy et al., *Removing the
//! HUD*, 2015, DOI 10.1145/2793107.2793120). Same argument as the in-world dialogue bubbles.
//!
//! Cosmetic and windowed-only: everything here runs on `Update` and touches nothing the pinned sim reads,
//! so it can never enter `snapshot_hash`.

use bevy::prelude::*;

use crate::ai::field::{FieldId, Stig, UNIT_THREAT_CHANNELS};
use crate::dungeon::Dungeon;
use crate::squad::Unit;
use crate::squad_ai::role::RoleId;

/// Field strength below which a cell is simply calm — keeps the whole floor from washing faintly.
const HEAT_FLOOR: f32 = 0.15;
/// Field strength that saturates the hottest band. A cell tracks the local crab COUNT, so ~3 crabs.
const HEAT_CEIL: f32 = 3.0;
/// Quantisation bands. Materials are built once; a cell picks one. Continuous tinting would mean a fresh
/// `StandardMaterial` per cell per frame — the orphaned-material-per-frame leak this codebase already had
/// once in `recolor_units`.
const BANDS: usize = 5;
/// Upper bound on simultaneously lit cells. The wash is a mood, not a data readout; capping it bounds
/// entity churn on a big level. Overflow is reported once rather than silently cropped.
const MAX_LIT_CELLS: usize = 768;
/// Height above the floor to float the wash, so it never z-fights the tile it sits on.
const HOVER: f32 = 0.02;
/// Cells are re-scanned every N frames. The fields evaporate over seconds; nobody can see 60 Hz here.
const RESCAN_FRAMES: u32 = 6;

pub struct PsiVisionPlugin;

impl Plugin for PsiVisionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PsiVisionState>()
            .add_systems(Startup, setup_psi_assets)
            .add_systems(Update, redraw_psi_vision);
    }
}

/// The pooled quads and their per-band materials, built once at startup.
#[derive(Resource)]
struct PsiVisionAssets {
    quad: Handle<Mesh>,
    bands: Vec<Handle<StandardMaterial>>,
}

#[derive(Resource, Default)]
struct PsiVisionState {
    frame: u32,
    /// Warn once, not every rescan, if a level ever exceeds the cell budget.
    warned_overflow: bool,
}

/// A pooled heat quad. Reused across rescans — spawned lazily, hidden rather than despawned.
#[derive(Component)]
struct PsiCell;

/// The field GROUPS psi-vision renders, each with its own hot RGB hue so the Psionic reads the swarm's
/// pheromonal STATE, not one undifferentiated heat: dread (what the squad fears), the muster ALARM bloom
/// (why a pack is boiling toward a casualty — the beat that otherwise reads as scripted aggression), and
/// the MEAT forage trails (where the swarm is drawn to feed). Order matches [`psi_group_channels`].
const PSI_GROUP_HUES: [[f32; 3]; 3] = [
    [0.9, 0.15, 0.9],  // dread — magenta
    [1.0, 0.28, 0.06], // alarm — red
    [0.15, 0.85, 0.5], // meat  — green
];

/// The stigmergy channels each render group folds (by `max`). Group 0 stays exactly the old dread set (the
/// channels a *unit* fears — never the squad's own THREAT_GUN/NOISE din). `&[FieldId::_]` const-promotes to
/// `'static`, and `&UNIT_THREAT_CHANNELS` coerces `&[_; 2] → &[_]`.
fn psi_group_channels(group: usize) -> &'static [FieldId] {
    match group {
        0 => &UNIT_THREAT_CHANNELS,
        1 => &[FieldId::ALARM],
        2 => &[FieldId::MEAT],
        _ => &[],
    }
}

fn setup_psi_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let quad = meshes.add(Rectangle::new(crate::dungeon::TILE_SIZE, crate::dungeon::TILE_SIZE));
    // One palette per render group: each ramps a dim → hot version of the group's hue, alpha rising with
    // intensity. Unlit so the wash reads at full strength in the dungeon's near-black corners, where the
    // danger actually matters. Stored FLAT, indexed `group * BANDS + band`.
    let mut bands = Vec::with_capacity(PSI_GROUP_HUES.len() * BANDS);
    for hue in PSI_GROUP_HUES {
        for i in 0..BANDS {
            let t = i as f32 / (BANDS - 1) as f32;
            let k = 0.4 + 0.6 * t; // brightness ramps with intensity
            bands.push(materials.add(StandardMaterial {
                base_color: Color::srgba(hue[0] * k, hue[1] * k, hue[2] * k, 0.10 + 0.30 * t),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            }));
        }
    }
    commands.insert_resource(PsiVisionAssets { quad, bands });
}

/// Map a raw field value to a band index, or `None` when the cell is calm.
fn band_of(heat: f32) -> Option<usize> {
    if heat < HEAT_FLOOR {
        return None;
    }
    let t = ((heat - HEAT_FLOOR) / (HEAT_CEIL - HEAT_FLOOR)).clamp(0.0, 1.0);
    Some(((t * (BANDS - 1) as f32).round() as usize).min(BANDS - 1))
}

/// Repaint the wash. Runs only while a living Psionic is in the squad — the sight belongs to the operative,
/// not to the player, so losing them loses it.
#[allow(clippy::too_many_arguments)]
fn redraw_psi_vision(
    mut commands: Commands,
    mut state: ResMut<PsiVisionState>,
    assets: Option<Res<PsiVisionAssets>>,
    stig: Option<Res<Stig>>,
    dungeon: Option<Res<Dungeon>>,
    psionics: Query<&RoleId, With<Unit>>,
    mut cells: Query<
        (&mut Transform, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility),
        With<PsiCell>,
    >,
) {
    let (Some(assets), Some(stig), Some(dungeon)) = (assets, stig, dungeon) else {
        return;
    };

    state.frame = state.frame.wrapping_add(1);
    if state.frame % RESCAN_FRAMES != 0 {
        return;
    }

    // No Psionic alive → no sight. Hide the pool rather than despawning it, so the sense can come back if
    // a future revive mechanic ever restores them.
    let has_psionic = psionics.iter().any(|r| *r == RoleId::Psionic);
    if !has_psionic {
        for (_, _, mut vis) in &mut cells {
            *vis = Visibility::Hidden;
        }
        return;
    }

    // Gather the lit cells. Each cell is coloured by its DOMINANT pheromone group (by raw magnitude): a
    // fresh ALARM bloom (~2) out-shouts ambient dread (~1) exactly where a pack is mustering, which is the
    // story to tell; forage MEAT (~0.5) surfaces only on real trails. Group 0 is still the danger a *unit*
    // is subject to — never the squad's own gunfire. `band_of` maps the winner's intensity to a brightness
    // band within its palette; the stored value is the FLAT material index (`group * BANDS + band`).
    let mut lit: Vec<(IVec2, usize)> = Vec::new();
    let mut overflowed = false;
    for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }
            let pos = dungeon.cell_center(cell);
            let mut best_mat: Option<usize> = None;
            let mut best_heat = 0.0f32;
            for group in 0..PSI_GROUP_HUES.len() {
                let heat = psi_group_channels(group)
                    .iter()
                    .map(|&ch| stig.sample(ch, &dungeon, pos))
                    .fold(0.0f32, f32::max);
                if heat > best_heat
                    && let Some(band) = band_of(heat)
                {
                    best_heat = heat;
                    best_mat = Some(group * BANDS + band);
                }
            }
            let Some(mat) = best_mat else { continue };
            if lit.len() == MAX_LIT_CELLS {
                overflowed = true;
                break;
            }
            lit.push((cell, mat));
        }
    }
    if overflowed && !state.warned_overflow {
        state.warned_overflow = true;
        warn!("psi_vision: more than {MAX_LIT_CELLS} lit cells; the wash is cropped this frame");
    }

    // Repaint the pool in place, growing it only when this frame needs more quads than exist.
    let mut painted = 0usize;
    for (mut tf, mut material, mut vis) in &mut cells {
        match lit.get(painted) {
            Some(&(cell, mat)) => {
                let p = dungeon.cell_center(cell);
                tf.translation = Vec3::new(p.x, HOVER, p.z);
                // Lie flat: the quad is authored in XY, the floor is XZ.
                tf.rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
                if material.0.id() != assets.bands[mat].id() {
                    material.0 = assets.bands[mat].clone();
                }
                *vis = Visibility::Visible;
                painted += 1;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for &(cell, mat) in &lit[painted..] {
        let p = dungeon.cell_center(cell);
        commands.spawn((
            PsiCell,
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(assets.bands[mat].clone()),
            Transform::from_xyz(p.x, HOVER, p.z)
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            Visibility::Visible,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calm_cells_are_not_lit() {
        assert_eq!(band_of(0.0), None);
        assert_eq!(band_of(HEAT_FLOOR - 0.001), None);
    }

    #[test]
    fn heat_maps_monotonically_onto_the_bands_and_saturates() {
        assert_eq!(band_of(HEAT_FLOOR), Some(0));
        assert_eq!(band_of(HEAT_CEIL), Some(BANDS - 1));
        assert_eq!(band_of(HEAT_CEIL * 10.0), Some(BANDS - 1), "a swarm must not index past the bands");
        let mut last = 0;
        for i in 0..=40 {
            let heat = HEAT_FLOOR + (HEAT_CEIL - HEAT_FLOOR) * (i as f32 / 40.0);
            let band = band_of(heat).expect("above the floor");
            assert!(band >= last, "band went backwards as danger rose");
            last = band;
        }
    }
}
