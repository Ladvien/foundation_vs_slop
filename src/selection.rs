//! Mouse + keyboard control of the squad (RTS/MOBA style):
//! - **Left-click a unit** → select it (green ring, "Move" cursor); a click elsewhere while units
//!   are selected issues a **move order** (units keep their selection). **Esc** deselects.
//! - **1–5** select that unit; **6** selects all.
//!
//! Selection/commands use a single cursor ray → ground-plane hit (no mesh picking needed): the hit
//! world point drives both unit-proximity selection and the move target. Green rings are drawn with
//! `gizmos.circle` (no per-unit ring entities to manage).

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};

use crate::dungeon::Dungeon;
use crate::pathfinding::{find_path, smooth_path};
use crate::squad::{MoveOrder, Selected, Unit, UnitIndex};

/// Click within this ground distance of a unit to select it.
const SELECT_RADIUS: f32 = 0.6;
/// Radius of the green selection ring.
const RING_RADIUS: f32 = 0.6;

/// Cells to try (in order) around a move target so a group fans into a small formation.
const FORMATION_SPIRAL: [(i32, i32); 13] = [
    (0, 0),
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (2, 0),
    (-2, 0),
    (0, 2),
    (0, -2),
];

pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                command_input,
                hotkey_select,
                draw_selection_rings,
                update_cursor,
            ),
        );
    }
}

/// Ground point under the cursor (y = 0 plane), or `None` if off-window / no camera ray.
fn cursor_ground_point(window: &Window, camera: &Camera, cam_tf: &GlobalTransform) -> Option<Vec3> {
    let cursor = window.cursor_position()?;
    let ray = camera.viewport_to_world(cam_tf, cursor).ok()?;
    let dist = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
    Some(ray.get_point(dist))
}

#[allow(clippy::too_many_arguments)]
fn command_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    dungeon: Res<Dungeon>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    units: Query<(Entity, &Transform), With<Unit>>,
    selected: Query<(Entity, &Transform), With<Selected>>,
) {
    // Esc clears the whole selection.
    if keys.just_pressed(KeyCode::Escape) {
        for (e, _) in &selected {
            commands.entity(e).remove::<Selected>();
        }
    }

    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, cam_tf) = *camera;
    let Some(point) = cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };

    // Did the click land on a unit? Pick the nearest within SELECT_RADIUS.
    let mut nearest: Option<(Entity, f32)> = None;
    for (e, tf) in &units {
        let d = (tf.translation - point).xz().length();
        if d < SELECT_RADIUS && nearest.is_none_or(|(_, best)| d < best) {
            nearest = Some((e, d));
        }
    }

    if let Some((unit, _)) = nearest {
        // Select just that unit.
        for (e, _) in &selected {
            commands.entity(e).remove::<Selected>();
        }
        commands.entity(unit).insert(Selected);
        return;
    }

    // Empty ground: if units are selected, order a move into a formation around the target.
    if selected.is_empty() {
        return;
    }
    let target = dungeon.world_to_cell(point);
    let goals = formation_cells(&dungeon, target, selected.iter().count());
    for ((entity, tf), goal) in selected.iter().zip(goals) {
        let start = dungeon.world_to_cell(tf.translation);
        if let Some(path) = find_path(&dungeon, start, goal) {
            commands.entity(entity).insert(MoveOrder {
                path: smooth_path(&dungeon, &path),
            });
        }
    }
}

/// Pick `n` distinct floor cells around `center` for a group's arrival formation.
fn formation_cells(dungeon: &Dungeon, center: IVec2, n: usize) -> Vec<IVec2> {
    let mut cells: Vec<IVec2> = FORMATION_SPIRAL
        .iter()
        .map(|&(dx, dy)| center + IVec2::new(dx, dy))
        .filter(|&c| dungeon.is_floor(c))
        .take(n)
        .collect();
    // If the area is cramped, pad with the center so every unit still gets a goal.
    while cells.len() < n {
        cells.push(center);
    }
    cells
}

fn hotkey_select(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    units: Query<(Entity, &UnitIndex), With<Unit>>,
    selected: Query<Entity, With<Selected>>,
) {
    const DIGITS: [KeyCode; 5] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
    ];

    let select_all = keys.just_pressed(KeyCode::Digit6);
    let single = DIGITS.iter().position(|&k| keys.just_pressed(k));
    if single.is_none() && !select_all {
        return;
    }

    // Any select command clears the current selection first.
    for e in &selected {
        commands.entity(e).remove::<Selected>();
    }
    for (e, idx) in &units {
        if select_all || single == Some(idx.0 as usize) {
            commands.entity(e).insert(Selected);
        }
    }
}

fn draw_selection_rings(mut gizmos: Gizmos, selected: Query<&Transform, With<Selected>>) {
    for tf in &selected {
        let iso = Isometry3d::new(
            tf.translation + Vec3::Y * 0.03,
            Quat::from_rotation_x(-FRAC_PI_2),
        );
        gizmos.circle(iso, RING_RADIUS, Color::srgb(0.1, 1.0, 0.2));
    }
}

/// Show the "Move" cursor while anything is selected, else the default arrow. Guarded so the
/// component is only re-inserted on a state change.
fn update_cursor(
    mut commands: Commands,
    window: Single<Entity, With<PrimaryWindow>>,
    selected: Query<(), With<Selected>>,
    mut last: Local<Option<bool>>,
) {
    let active = !selected.is_empty();
    if *last == Some(active) {
        return;
    }
    *last = Some(active);
    let icon = if active {
        SystemCursorIcon::Move
    } else {
        SystemCursorIcon::Default
    };
    commands.entity(*window).insert(CursorIcon::from(icon));
}
