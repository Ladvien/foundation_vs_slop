//! Line-of-sight fog of war (3-state). Every dungeon cell is `Unseen` (black), `Explored` (seen
//! before, remembered dim), or `Visible` (in a unit's live line of sight, fully lit). Each time
//! the squad crosses cell boundaries we recompute the visible set as the union of every unit's
//! LOS disc (walls block sight — see `Dungeon::line_of_sight`); cells that leave LOS fall back to
//! `Explored`. Reveal of a cell's tiles (`Visibility::Hidden`→`Visible`) is one-way; the
//! bright/dim distinction is a floor-material swap (walls stay lit once seen, so they never fight
//! the occlusion system's ownership of wall materials).

use std::collections::HashMap;

use bevy::prelude::*;

use crate::dungeon::{Dungeon, FloorMaterials, Tile, Wall};
use crate::squad::Unit;

/// How many cells out from a unit can be seen (subject to line of sight).
const VISION_RADIUS: i32 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CellVis {
    Unseen,
    Explored,
    Visible,
}

/// Per-cell visibility memory plus a cell → tile-entities index for cheap reveals.
#[derive(Resource)]
pub struct FogGrid {
    width: usize,
    vis: Vec<CellVis>,
    /// Tile entities (floor + walls) keyed by grid cell. Built once, lazily.
    cell_tiles: HashMap<IVec2, Vec<Entity>>,
    /// Sorted unit cells from last recompute — skip work when nothing crossed a boundary.
    last_cells: Vec<IVec2>,
    /// Set the frame the visible set changed, so the floor-material pass only runs then.
    dirty: bool,
}

impl FogGrid {
    fn new(width: usize, height: usize) -> Self {
        FogGrid {
            width,
            vis: vec![CellVis::Unseen; width * height],
            cell_tiles: HashMap::new(),
            last_cells: Vec::new(),
            dirty: false,
        }
    }

    #[inline]
    fn index(&self, c: IVec2) -> usize {
        c.y as usize * self.width + c.x as usize
    }

    /// Is cell `c` in a unit's *live* line of sight right now? (Not merely explored-and-remembered.)
    /// This is the partial-observability query other systems use to hide/target enemies — hidden
    /// units outside current LOS are the defining property of an RTS fog-of-war (Yang, Xie & Peng,
    /// "Fuzzy Theory Based Single Belief State Generation for Partially Observable Real-Time Strategy
    /// Games", IEEE Access 2019, DOI 10.1109/access.2019.2923419).
    pub fn visible_at(&self, c: IVec2) -> bool {
        if c.x < 0 || c.y < 0 || c.x as usize >= self.width {
            return false;
        }
        let idx = self.index(c);
        idx < self.vis.len() && self.vis[idx] == CellVis::Visible
    }
}

pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        let dungeon = app
            .world()
            .get_resource::<Dungeon>()
            .expect("FogPlugin requires DungeonPlugin to be registered first");
        let fog = FogGrid::new(dungeon.width, dungeon.height);
        app.insert_resource(fog)
            .add_systems(Update, (update_los, apply_floor_fog).chain());
    }
}

/// Recompute the visible set from every unit's LOS disc when the squad has moved between cells.
fn update_los(
    dungeon: Res<Dungeon>,
    mut fog: ResMut<FogGrid>,
    units: Query<&Transform, With<Unit>>,
    tiles: Query<(Entity, &Tile)>,
    mut visibility: Query<&mut Visibility>,
) {
    let fog = &mut *fog;

    // Build the cell → entities index once, after the tiles have spawned.
    if fog.cell_tiles.is_empty() {
        for (entity, tile) in &tiles {
            fog.cell_tiles.entry(tile.cell).or_default().push(entity);
        }
    }

    // Current unit cells (sorted for a stable comparison against last frame).
    let mut cells: Vec<IVec2> = units
        .iter()
        .map(|t| dungeon.world_to_cell(t.translation))
        .collect();
    cells.sort_unstable_by_key(|c| (c.x, c.y));
    if cells == fog.last_cells {
        fog.dirty = false;
        return;
    }
    fog.last_cells = cells.clone();
    fog.dirty = true;

    // Everything currently visible falls back to explored; LOS below re-lights what still shows.
    for v in fog.vis.iter_mut() {
        if *v == CellVis::Visible {
            *v = CellVis::Explored;
        }
    }

    for &uc in &cells {
        for dy in -VISION_RADIUS..=VISION_RADIUS {
            for dx in -VISION_RADIUS..=VISION_RADIUS {
                if dx * dx + dy * dy > VISION_RADIUS * VISION_RADIUS {
                    continue;
                }
                let c = uc + IVec2::new(dx, dy);
                if !dungeon.is_floor(c) || !dungeon.line_of_sight(uc, c) {
                    continue;
                }
                let i = fog.index(c);
                let was = fog.vis[i];
                fog.vis[i] = CellVis::Visible;
                // First sighting: reveal this cell's tiles (floor + walls) permanently.
                if was == CellVis::Unseen && let Some(entities) = fog.cell_tiles.get(&c) {
                    for &entity in entities {
                        if let Ok(mut vis) = visibility.get_mut(entity) {
                            *vis = Visibility::Visible;
                        }
                    }
                }
            }
        }
    }
}

/// After a visibility change, tint floor tiles: bright where a unit currently sees them, dim where
/// only explored. Walls are left to the occlusion system (they stay lit once revealed).
fn apply_floor_fog(
    mut fog: ResMut<FogGrid>,
    mats: Res<FloorMaterials>,
    mut floors: Query<(&Tile, &mut MeshMaterial3d<StandardMaterial>), (With<Tile>, Without<Wall>)>,
) {
    if !fog.dirty {
        return;
    }
    fog.dirty = false;
    for (tile, mut material) in &mut floors {
        let want = match fog.vis[fog.index(tile.cell)] {
            CellVis::Visible => &mats.bright,
            CellVis::Explored | CellVis::Unseen => &mats.dim,
        };
        if material.0.id() != want.id() {
            material.0 = want.clone();
        }
    }
}
