//! **The minimap** — topology of what you have seen, and only while a sensor is live.
//!
//! # It is gated, and that is the design
//!
//! Deploying a sensor (`crate::sensor`, `V`) fades this in for [`crate::sensor::SENSOR_DURATION`]
//! seconds; the cooldown is longer than the duration, so there is always a window without it. The
//! reasoning — and the two papers it rests on — lives in `crate::sensor`'s module header, because the
//! *cost* is the interesting half. This module is only the rendering.
//!
//! # What it draws, and what it refuses to
//!
//! It reads `fog::FogGrid` — the three-state memory the game already keeps (`Unseen` / `Explored` /
//! `Visible`) — and paints **walls, floor and the way out**. It draws:
//!
//! - explored floor, dim; live-visible floor, bright
//! - the operatives, and which of them are selected
//! - the extraction point
//!
//! It **never draws a creature.** Not crabs, not the watcher, not a nest. The fog already withholds
//! enemies outside live line of sight (`fog::visible_at` is what `laser::fire_laser` targets
//! through), and that withholding is the spatial-uncertainty axis McCall et al. 2022 measure dread
//! from. A minimap with enemy blips would hand back exactly what the fog exists to take away, and it
//! would do it *while* claiming to be a spatial aid.
//!
//! It also does not reveal unexplored cells. A sensor widens what the *map* shows of ground you have
//! walked; it is not a wallhack. `sensor::SENSOR_RADIUS` bounds that widening.
//!
//! # Why it is one image and not a grid of nodes
//!
//! A 96×96 dungeon is 9,216 cells. As UI nodes that is 9,216 entities re-laid-out by Taffy every
//! frame the map is up. It is instead **one `ImageNode` over a CPU-written RGBA texture**, the same
//! shape `dialogue::bubble` uses for its balloons — one entity, one upload, no layout cost.
//!
//! `ImageNode::default()` is an invisible 1×1 transparent texture (`docs/ui.md` §5, trap 6), so the
//! handle is always constructed explicitly.
//!
//! Windowed-only, `Update` only, reads sim state and writes none — nothing here reaches
//! `snapshot_hash`.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::layout::{self, HudRegions, Region};
use super::state::{despawn_scoped, AppState};
use super::theme::{UiTheme, Z_HUD};
use crate::containment::ExtractionZone;
use crate::dungeon::Dungeon;
use crate::fog::FogGrid;
use crate::sensor::Sensor;
use crate::squad::{Selected, Unit};

/// Side length of the rendered texture, in pixels. One pixel per cell would be unreadable at any
/// sane panel size, so cells are drawn as [`CELL_PX`] blocks.
const MAP_PX: u32 = 192;

/// Pixels per dungeon cell in the texture.
const CELL_PX: u32 = 2;

/// On-screen size of the panel, in logical px.
const PANEL_PX: f32 = 176.0;

/// Root marker.
#[derive(Component)]
pub struct MinimapRoot;

/// The image node whose texture is rewritten as the fog changes.
#[derive(Component)]
pub struct MinimapImage(pub Handle<Image>);

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::InGame),
            spawn_minimap.after(layout::spawn_frame),
        )
        .add_systems(OnExit(AppState::InGame), despawn_scoped::<MinimapRoot>)
        .add_systems(
            Update,
            (show_while_sensed, redraw_minimap)
                .chain()
                .run_if(in_state(AppState::InGame))
                .run_if(in_state(crate::session::RunState::Active)),
        );
    }
}

fn spawn_minimap(
    mut commands: Commands,
    theme: Res<UiTheme>,
    regions: Res<HudRegions>,
    mut images: ResMut<Assets<Image>>,
) {
    let handle = images.add(blank_map());
    // MidRight: one of the four regions that were empty during play. `docs/ui.md` §1.2 and van den
    // Berg's crowding model both argue for spending an *empty* region rather than stacking a third
    // panel into an occupied corner.
    let panel = (
        MinimapRoot,
        Node {
            width: Val::Px(PANEL_PX),
            height: Val::Px(PANEL_PX),
            border: UiRect::all(Val::Px(1.0)),
            display: Display::None, // hidden until a sensor is live
            ..default()
        },
        BackgroundColor(theme.panel),
        super::widgets::border_all(theme.panel_border),
        GlobalZIndex(Z_HUD),
        Pickable::IGNORE,
    );
    let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::MidRight, panel) else {
        error!("minimap: no layout frame at spawn — the sensor will reveal nothing");
        return;
    };
    ec.with_children(|p| {
        p.spawn((
            MinimapImage(handle.clone()),
            // NEVER `ImageNode::default()` — that is a 1×1 transparent texture and the map would be
            // invisible with no error anywhere (`docs/ui.md` §5, trap 6).
            ImageNode::new(handle),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            Pickable::IGNORE,
        ));
    });
}

/// A fully transparent square, the texture's starting state.
fn blank_map() -> Image {
    Image::new_fill(
        Extent3d { width: MAP_PX, height: MAP_PX, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

/// Show the panel exactly while at least one sensor is live.
fn show_while_sensed(
    sensors: Query<&Sensor>,
    mut panels: Query<&mut Node, With<MinimapRoot>>,
) {
    let want = if sensors.iter().next().is_some() { Display::Flex } else { Display::None };
    for mut node in &mut panels {
        if node.display != want {
            node.display = want;
        }
    }
}

/// Which cells a set of live sensors covers, as a closure over the Chebyshev discs.
///
/// Pure and separate so the coverage rule is testable: a cell is on the map if it is within
/// `SENSOR_RADIUS` of *any* live drone. Chebyshev (a square) rather than Euclidean because the
/// dungeon is a square grid and a circular reveal on a square grid produces a ragged edge that reads
/// as a rendering artefact rather than as a range.
fn covered(cell: IVec2, sensors: &[IVec2], radius: i32) -> bool {
    sensors
        .iter()
        .any(|s| (cell.x - s.x).abs() <= radius && (cell.y - s.y).abs() <= radius)
}

#[allow(clippy::too_many_arguments)]
fn redraw_minimap(
    theme: Res<UiTheme>,
    dungeon: Option<Res<Dungeon>>,
    fog: Option<Res<FogGrid>>,
    sensors: Query<&Sensor>,
    units: Query<(&Transform, Option<&Selected>), With<Unit>>,
    zones: Query<(&ExtractionZone, &Transform)>,
    panels: Query<&Node, With<MinimapRoot>>,
    images: Query<&MinimapImage>,
    mut assets: ResMut<Assets<Image>>,
) {
    // Nothing to draw while hidden — and redrawing a hidden 192² texture every frame would be the
    // most expensive no-op in the UI.
    if panels.iter().all(|n| n.display == Display::None) {
        return;
    }
    let (Some(dungeon), Some(fog)) = (dungeon, fog) else { return };
    let live: Vec<IVec2> = sensors.iter().map(|s| s.cell).collect();
    if live.is_empty() {
        return;
    }
    let Some(handle) = images.iter().next() else { return };
    let Some(mut image) = assets.get_mut(&handle.0) else { return };

    // Centre the view on the first sensor, so the panel shows the neighbourhood it revealed rather
    // than a whole-dungeon thumbnail in which nothing is legible.
    let centre = live[0];
    let span = (MAP_PX / CELL_PX) as i32; // cells across the texture
    let half = span / 2;

    let px = |c: Color| -> [u8; 4] {
        let s = c.to_srgba();
        [
            (s.red * 255.0) as u8,
            (s.green * 255.0) as u8,
            (s.blue * 255.0) as u8,
            (s.alpha * 255.0) as u8,
        ]
    };
    let clear = [0u8, 0, 0, 0];
    let explored = px(theme.text_muted.with_alpha(0.35));
    let visible = px(theme.accent.with_alpha(0.55));
    let unit_ink = px(theme.text);
    let selected_ink = px(theme.accent);
    let exit_ink = px(theme.accent.with_alpha(0.95));

    let Some(data) = image.data.as_mut() else { return };
    data.fill(0);

    let mut put = |gx: i32, gy: i32, rgba: [u8; 4]| {
        // Grid cell → texture block. Bounds-checked rather than indexed: `MAP_PX`/`CELL_PX` are
        // constants today, but the repo's no-panic rule does not care why an index went out of range.
        let bx = (gx - centre.x + half) * CELL_PX as i32;
        let by = (gy - centre.y + half) * CELL_PX as i32;
        for oy in 0..CELL_PX as i32 {
            for ox in 0..CELL_PX as i32 {
                let (x, y) = (bx + ox, by + oy);
                if x < 0 || y < 0 || x >= MAP_PX as i32 || y >= MAP_PX as i32 {
                    continue;
                }
                let i = ((y as usize) * MAP_PX as usize + x as usize) * 4;
                if let Some(slot) = data.get_mut(i..i + 4) {
                    slot.copy_from_slice(&rgba);
                }
            }
        }
    };

    // --- Terrain: explored floor, brighter where currently in line of sight. ---
    for gy in (centre.y - half)..=(centre.y + half) {
        for gx in (centre.x - half)..=(centre.x + half) {
            let cell = IVec2::new(gx, gy);
            // Three independent gates, and each one is load-bearing: the sensor's range, the fog's
            // memory (never reveal ground the squad has not walked), and whether it is floor at all.
            if !covered(cell, &live, crate::sensor::SENSOR_RADIUS) {
                continue;
            }
            if !fog.seen_at(cell) || !dungeon.is_floor(cell) {
                put(gx, gy, clear);
                continue;
            }
            put(gx, gy, if fog.visible_at(cell) { visible } else { explored });
        }
    }

    // --- The way out, then the operatives on top of it. ---
    for (_, tf) in &zones {
        let c = dungeon.world_to_cell(tf.translation);
        if covered(c, &live, crate::sensor::SENSOR_RADIUS) {
            put(c.x, c.y, exit_ink);
        }
    }
    for (tf, selected) in &units {
        let c = dungeon.world_to_cell(tf.translation);
        if covered(c, &live, crate::sensor::SENSOR_RADIUS) {
            put(c.x, c.y, if selected.is_some() { selected_ink } else { unit_ink });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_is_a_square_disc_around_each_sensor() {
        let s = [IVec2::new(10, 10)];
        assert!(covered(IVec2::new(10, 10), &s, 3), "the sensor's own cell");
        assert!(covered(IVec2::new(13, 13), &s, 3), "the corner of the square is in range");
        assert!(!covered(IVec2::new(14, 10), &s, 3), "one past the edge is out");
    }

    #[test]
    fn two_sensors_cover_the_union() {
        let s = [IVec2::new(0, 0), IVec2::new(40, 40)];
        assert!(covered(IVec2::new(2, 2), &s, 5));
        assert!(covered(IVec2::new(38, 42), &s, 5));
        assert!(!covered(IVec2::new(20, 20), &s, 5), "the gap between them is not covered");
    }

    #[test]
    fn no_sensors_cover_nothing() {
        // The gate that makes this a verb rather than furniture. If an empty sensor list covered
        // everything, the map would silently become permanent — the exact failure `sensor`'s
        // cooldown test guards from the other side.
        assert!(!covered(IVec2::ZERO, &[], 100));
    }

    #[test]
    fn the_texture_is_a_whole_number_of_cells() {
        // A fractional cell would put a half-block seam down one edge of every map, which reads as a
        // rendering bug rather than as a boundary.
        assert_eq!(MAP_PX % CELL_PX, 0);
        assert!(MAP_PX / CELL_PX >= 2 * crate::sensor::SENSOR_RADIUS as u32,
            "the texture must be wide enough to show the whole revealed disc");
    }

    #[test]
    fn a_blank_map_is_fully_transparent() {
        // The panel sits over the world, so any non-zero starting alpha would show as a grey square
        // before the first redraw.
        let img = blank_map();
        let data = img.data.expect("filled image has data");
        assert_eq!(data.len(), (MAP_PX * MAP_PX * 4) as usize);
        assert!(data.iter().all(|b| *b == 0));
    }
}
