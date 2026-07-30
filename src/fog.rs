//! Line-of-sight fog of war (3-state). Every dungeon cell is `Unseen` (black), `Explored` (seen
//! before, remembered dim), or `Visible` (in a unit's live line of sight, fully lit). Each time
//! the squad crosses cell boundaries we recompute the visible set as the union of every unit's
//! LOS disc (walls block sight — see `Dungeon::line_of_sight`); cells that leave LOS fall back to
//! `Explored`. Reveal of a cell's tiles (`Visibility::Hidden`→`Visible`) is one-way; the
//! bright/dim distinction is a floor-material swap; walls stay lit once seen and fog never touches
//! wall materials (the dungeon's knee-wall squash owns camera-facing walls).

use std::collections::HashMap;

use bevy::prelude::*;

use crate::dungeon::{Dungeon, FloorMaterials, Tile, Wall};
use crate::squad::Unit;

/// How many cells out from a unit can be seen (subject to line of sight). `pub` so the smiley watcher's
/// gaze range reuses it (single source of truth — see `enemy::LOOK_RANGE`) instead of a copied literal.
pub const VISION_RADIUS: i32 = 8;

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
        crate::util::row_major(c, self.width)
    }

    /// Every cell explored, none currently visible — the "seen but unwatched" state a fruit body pins in
    /// (`mycelia::fruit`). Test-only: in the game `update_los` is the sole writer of this grid.
    #[cfg(test)]
    pub(crate) fn all_explored(width: usize, height: usize) -> Self {
        let mut grid = FogGrid::new(width, height);
        grid.vis = vec![CellVis::Explored; width * height];
        grid
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

    /// Has cell `c` *ever* been in a unit's line of sight (Explored or Visible)? This is the permanent,
    /// one-way "explored" memory — never demoted back to Unseen — the same reveal the floor/wall tiles
    /// use. Furniture reveal keys off this so a room seen once stays furnished after the squad leaves.
    pub fn seen_at(&self, c: IVec2) -> bool {
        if c.x < 0 || c.y < 0 || c.x as usize >= self.width {
            return false;
        }
        let idx = self.index(c);
        idx < self.vis.len() && self.vis[idx] != CellVis::Unseen
    }
}

/// System set for `update_los`, the sole writer of [`FogGrid`]. Its `FixedUpdate` readers —
/// `brain::think` (`seen_by_squad`) and `laser::fire_laser` (the LOS target gate) — order themselves
/// `.after(LosWritten)` so they read the current tick's visibility, not last tick's. Without this the
/// multithreaded executor is free to run a reader before the writer, so aggro/auto-aim would engage or
/// drop one fixed tick late on the tick the squad first sees (or loses sight of) a target.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LosWritten;

pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        // Sized per run, not at plugin build: the `Dungeon` is now generated on
        // `OnEnter(RunState::Active)` (FVS-A-5), so this grid is rebuilt for each expedition's map.
        app.add_systems(
            OnEnter(crate::session::RunState::Active),
            size_fog_grid.in_set(crate::session::RunBuild::Grids),
        )
            // `update_los` is PINNED gameplay: the visibility grid it writes gates laser targeting and
            // the crabs' `seen_by_squad` perception, so it must advance on the fixed timestep (and at the
            // same rate as the systems that read it, or fast-forward would change what's visible when).
            .add_systems(FixedUpdate, update_los.in_set(LosWritten).distributive_run_if(in_state(crate::session::RunState::Active)))
            // `apply_floor_fog` only tints floor tiles from that grid — cosmetic, so it stays on `Update`.
            .add_systems(Update, apply_floor_fog.distributive_run_if(in_state(crate::session::RunState::Active)));
    }
}

/// Size this run's fog grid to its dungeon.
fn size_fog_grid(mut commands: Commands, dungeon: Res<Dungeon>) {
    commands.insert_resource(FogGrid::new(dungeon.width, dungeon.height));
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
        // Unit cells unchanged this sub-step: nothing to recompute. Do NOT clear `dirty` here —
        // FixedUpdate can run several sub-steps per rendered frame, and an earlier sub-step in this
        // same frame may have set `dirty` for a real visibility change. `apply_floor_fog` (Update)
        // is the single consumer and clears it once per frame after the material swap.
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
                // A cell is seen when it is floor and the straight line from the unit isn't wall-blocked.
                // (Mold no longer occludes sight — the mold→LOS "soft cover" coupling was removed; mold
                // now only dims the light field, never the fog. So a unit always reveals the floor it and
                // its neighbours stand on, even in thick mold.)
                // NOTE: this grid is not reveal-only. `FogGrid` is also read by `laser::fire_laser`
                // and by the `seen_by_squad` input to `ai::brain` (see the `LosWritten` doc above),
                // so the lenient rule chosen here for the *reveal* reaches them too. `fire_laser`
                // therefore carries its own explicit `Dungeon::line_of_sight` test — targeting needs
                // the strict corner rule even though the reveal must not have it. `seen_by_squad`
                // deliberately stays on this grid: "the squad can see me" is exactly the reveal
                // relation, so leniency is correct there. Anything else added downstream of this
                // grid must make the same choice consciously.
                if !dungeon.is_floor(c) || !dungeon.line_of_sight_reveal(uc, c) {
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
/// only explored. Walls are handled by the dungeon's knee-wall squash and stay lit once revealed, so
/// this query is floor-only (`Without<Wall>`).
fn apply_floor_fog(
    mut fog: ResMut<FogGrid>,
    mats: Res<FloorMaterials>,
    // Needed for the cell's surface biome: the bright/dim pair is per-biome, so swapping on fog state
    // alone would repaint a concrete floor as motel carpet the moment a unit looked at it.
    dungeon: Res<Dungeon>,
    mut floors: Query<(&Tile, &mut MeshMaterial3d<StandardMaterial>), (With<Tile>, Without<Wall>)>,
) {
    if !fog.dirty {
        return;
    }
    fog.dirty = false;
    for (tile, mut material) in &mut floors {
        let visible = matches!(fog.vis[fog.index(tile.cell)], CellVis::Visible);
        let want = mats.pick(dungeon.biome(tile.cell), visible);
        if material.0.id() != want.id() {
            material.0 = want.clone();
        }
    }
}

/// Conceal every `T` standing on a cell the squad cannot currently SEE — the one fog-of-war visibility
/// pass, generic over the marker that selects which actors it governs.
///
/// This is the partial observability that defines an RTS (Yang, Xie & Peng, "Fuzzy Theory Based Single
/// Belief State Generation for Partially Observable Real-Time Strategy Games", IEEE Access 2019,
/// DOI 10.1109/access.2019.2923419). It existed as three byte-identical copies — enemies, gib chunks,
/// and the SCP-999 blob — differing only in that marker; register `hide_in_fog::<Marker>` instead of
/// writing a fourth.
///
/// **Cosmetic, and safe on `Update`.** It reads `FogGrid` (written on `FixedUpdate` by `update_los`) and
/// writes only `Visibility`, which enters no replay oracle: `snapshot_hash` folds `(Transform, Health)`
/// and `gib_hash` folds `GibKey`/`Transform`/`Carryable`/ring order — never visibility. It never
/// despawns or reorders, which WOULD perturb the `GibRing` folds. Driven every frame rather than off the
/// fog dirty flag, because actors move in and out of line of sight while the squad's own cell is
/// unchanged. Hiding the root propagates to child meshes (faces, gel, eye billboards).
pub fn hide_in_fog<T: Component>(
    fog: Res<FogGrid>,
    dungeon: Res<Dungeon>,
    mut actors: Query<(&Transform, &mut Visibility), With<T>>,
) {
    for (tf, mut vis) in &mut actors {
        let want = if fog.visible_at(dungeon.world_to_cell(tf.translation)) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
    }
}
