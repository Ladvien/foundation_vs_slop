//! Fog of war. Every dungeon tile starts hidden; cells within a radius of the player
//! are revealed permanently as the agent explores. Combined with the player's torch
//! (low global ambient), revealed-but-distant areas stay dim while the immediate
//! surroundings are brightly lit — the classic explored/visible gradient.

use bevy::prelude::*;

use crate::dungeon::{Dungeon, Tile};
use crate::player::Player;

/// How many cells out from the player get revealed (roughly a torch's reach).
const REVEAL_RADIUS: i32 = 5;

/// Per-cell reveal memory. `true` = the player has seen this cell at least once.
#[derive(Resource)]
struct FogGrid {
    width: usize,
    revealed: Vec<bool>,
}

impl FogGrid {
    fn new(width: usize, height: usize) -> Self {
        FogGrid {
            width,
            revealed: vec![false; width * height],
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
    mut tiles: Query<(&Tile, &mut Visibility)>,
) {
    let center = dungeon.world_to_cell(player.translation);

    // Reveal the disc of cells around the player; track whether anything new appeared.
    let mut newly_revealed = false;
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
            if !fog.revealed[i] {
                fog.revealed[i] = true;
                newly_revealed = true;
            }
        }
    }

    // Only touch the tile entities when the revealed set actually grew.
    if newly_revealed {
        for (tile, mut visibility) in &mut tiles {
            let want = if fog.revealed[fog.index(tile.cell)] {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
            if *visibility != want {
                *visibility = want;
            }
        }
    }
}
