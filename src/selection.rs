//! Mouse control of the squad (RTS/MOBA style). The **whole squad is always selected** — every unit
//! wears the green ring and `keep_squad_selected` re-selects any that lack it (a fresh spawn, say), so
//! the player never has to select and can't deselect. A **left-click** issues a **move order** to the
//! whole group toward one shared destination.
//!
//! Commands use a single cursor ray → ground-plane hit (no mesh picking needed): the hit world point is
//! the move target. Green rings are drawn with `gizmos.circle` (no per-unit ring entities to manage).

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};

use std::sync::Arc;

use crate::audio::Sfx;
use crate::dungeon::Dungeon;
use crate::flowfield::FlowField;
use crate::squad::{MoveOrder, Selected, Unit};

/// Radius of the green selection ring.
const RING_RADIUS: f32 = 0.6;

/// Outward ring search bound (in cells) for snapping a click on void/wall to the nearest floor so
/// the group still has a reachable goal. Beyond this a click is treated as "nowhere to go".
const SNAP_MAX_RING: i32 = 8;

pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                // Guarantee the whole squad is selected before anything reads the selection this frame.
                keep_squad_selected.before(command_input),
                command_input,
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

/// The squad is always fully selected — every command targets the whole group. This inserts `Selected`
/// on any `Unit` that lacks it (startup, or a freshly spawned unit), so the player never selects and
/// can't deselect. Runs before `command_input` so orders + rings see the full squad this frame.
fn keep_squad_selected(mut commands: Commands, units: Query<Entity, (With<Unit>, Without<Selected>)>) {
    for e in &units {
        commands.entity(e).insert(Selected);
    }
}

pub fn command_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    dungeon: Res<Dungeon>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    selected: Query<(Entity, &Transform), With<Selected>>,
    mut sfx: MessageWriter<Sfx>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, cam_tf) = *camera;
    let Some(point) = cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };

    // Left-click = order the whole (always-selected) squad toward one shared destination.
    if selected.is_empty() {
        return;
    }
    // Snap the click to a floor cell, then build ONE flow field the whole selection shares. Units
    // flow to the same goal and ORCA packs them into a blob — no per-unit goal cells to fight over.
    let raw = dungeon.world_to_cell(point);
    let Some(goal) = nearest_floor(&dungeon, raw) else {
        warn!("move order ignored: no floor within {SNAP_MAX_RING} cells of the click");
        sfx.write(Sfx::Invalid);
        return;
    };
    let Some(field) = FlowField::build(&dungeon, goal) else {
        warn!("move order ignored: could not build a flow field to {goal:?}");
        sfx.write(Sfx::Invalid);
        return;
    };
    let field = Arc::new(field);
    let mut ordered_any = false;
    for (entity, tf) in &selected {
        // Skip a unit that can't reach the goal at all (different connected component) — loud, not
        // a silent stall.
        let start = dungeon.world_to_cell(tf.translation);
        if !field.reachable(start) {
            warn!("unit at {start:?} cannot reach goal {goal:?}; order skipped for it");
            continue;
        }
        commands
            .entity(entity)
            .insert(MoveOrder::new(field.clone()));
        ordered_any = true;
    }
    // One acknowledgement for the whole order (not one per unit).
    if ordered_any {
        sfx.write(Sfx::MoveOrder);
    }
}

/// Nearest floor cell to `c` by an outward ring search, so a click on a wall/void still yields a
/// reachable goal. Bounded by [`SNAP_MAX_RING`] so a click deep in the void fails loudly.
fn nearest_floor(dungeon: &Dungeon, c: IVec2) -> Option<IVec2> {
    if dungeon.is_floor(c) {
        return Some(c);
    }
    for r in 1..=SNAP_MAX_RING {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue; // ring perimeter only
                }
                let cell = IVec2::new(c.x + dx, c.y + dy);
                if dungeon.is_floor(cell) {
                    return Some(cell);
                }
            }
        }
    }
    None
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
