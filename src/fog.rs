//! Fog of war. Every dungeon tile starts hidden; cells within a radius of the player are
//! revealed permanently as the agent explores. Reveal is one-way, so the system only ever
//! flips *newly*-revealed cells' tiles to visible — it never rescans the whole grid, so its
//! per-frame cost is proportional to what was just discovered, not the dungeon size.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::dungeon::{Dungeon, Tile};
use crate::player::Player;

/// How many cells out from the player get revealed (roughly a torch's reach).
const REVEAL_RADIUS: i32 = 5;

/// Per-cell reveal memory plus a cell → tile-entities index for cheap reveals.
#[derive(Resource)]
struct FogGrid {
    width: usize,
    revealed: Vec<bool>,
    /// Tile entities (floor + walls) keyed by their grid cell. Built once, lazily.
    cell_tiles: HashMap<IVec2, Vec<Entity>>,
}

impl FogGrid {
    fn new(width: usize, height: usize) -> Self {
        FogGrid {
            width,
            revealed: vec![false; width * height],
            cell_tiles: HashMap::new(),
        }
    }

    #[inline]
    fn index(&self, c: IVec2) -> usize {
        c.y as usize * self.width + c.x as usize
    }
}

pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        // Dungeon is inserted in DungeonPlugin::build, so its dimensions are available
        // here as long as DungeonPlugin is registered first.
        let dungeon = app
            .world()
            .get_resource::<Dungeon>()
            .expect("FogPlugin requires DungeonPlugin to be registered first");
        let fog = FogGrid::new(dungeon.width, dungeon.height);
        app.insert_resource(fog).add_systems(Update, update_fog);
    }
}

fn update_fog(
    dungeon: Res<Dungeon>,
    mut fog: ResMut<FogGrid>,
    player: Single<&Transform, With<Player>>,
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

    let center = dungeon.world_to_cell(player.translation);

    // Reveal the disc of cells around the player; make each newly-revealed cell's tiles
    // visible immediately (reveal is permanent, so tiles are never hidden again).
    for dy in -REVEAL_RADIUS..=REVEAL_RADIUS {
        for dx in -REVEAL_RADIUS..=REVEAL_RADIUS {
            if dx * dx + dy * dy > REVEAL_RADIUS * REVEAL_RADIUS {
                continue;
            }
            let cell = center + IVec2::new(dx, dy);
            if !dungeon.in_bounds(cell) {
                continue;
            }
            let i = fog.index(cell);
            if fog.revealed[i] {
                continue;
            }
            fog.revealed[i] = true;
            if let Some(entities) = fog.cell_tiles.get(&cell) {
                for &entity in entities {
                    if let Ok(mut vis) = visibility.get_mut(entity) {
                        *vis = Visibility::Visible;
                    }
                }
            }
        }
    }
}
