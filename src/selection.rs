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
use crate::dialogue::ConversationLock;
use crate::dungeon::Dungeon;
use crate::flowfield::FlowField;
use crate::containment::{ArmedTool, DeviceSupply, QuarantineSupply, TargetId};
use crate::squad::{MoveOrder, Selected, Unit};

/// Radius of the green selection ring.
const RING_RADIUS: f32 = 0.6;

/// Outward ring search bound (in cells) for snapping a click on void/wall to the nearest floor so
/// the group still has a reachable goal. Beyond this a click is treated as "nowhere to go".
const SNAP_MAX_RING: i32 = 8;

/// How far from the cursor a verb will still claim a target (world units).
///
/// Deliberately independent of — and more forgiving than — the verbs' *reach*. This is a UI affordance
/// ("did you mean that creature?"); reach is the mechanic ("are you close enough?"). Conflating them
/// would mean a click one pixel off reads as out-of-range, which teaches the player the wrong lesson
/// about why a throw failed.
const AIM_TOLERANCE: f32 = 1.2;

/// Aim tolerance for a verb whose mechanic reaches `reach` world units.
///
/// **The affordance must never be tighter than the mechanic**, which is what the doc above promises and
/// what the shipped constants violated. Measured 2026-07-28 from a play log: `AIM_TOLERANCE` is `1.2`
/// while `cap_reach` is `1.5`, `device_reach` is `2.5` and `quarantine_radius` is `3.0` — so the *aim*
/// was the binding constraint on all three verbs. A player standing well inside cap range, clicking
/// 1.3 m from a nest's centre, got `no uncapped nest under the cursor` and no way to tell whether they
/// had missed or were out of range. The log that surfaced this shows **nine** consecutive failed cap
/// attempts.
///
/// Deriving it from the reach rather than raising the constant keeps one rule instead of a number that
/// has to be re-checked every time a reach is tuned — and `max` preserves the 1.2 floor for any verb
/// whose reach is smaller, so this only ever loosens, never tightens.
fn aim_tolerance(reach: f32) -> f32 {
    reach.max(AIM_TOLERANCE)
}

/// A request to arm a verb from a source that is **not** the keyboard — today, the clickable verb
/// bar (`crate::ui::verb_bar`).
///
/// Routed as a message on purpose. [`arm_tool_input`] is the single writer of [`ArmedTool`], and a
/// UI panel reaching in to write it directly would be a second writer with its own copy of the
/// toggle-to-disarm rule and its own chance to forget the `DebugCaptureActive` stand-down. This is
/// the same discipline `ui::debrief`'s F10 dev-victory follows: send the intent, let the one owner
/// apply it.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmRequest {
    /// Arm this verb, or disarm it if it is already armed.
    Toggle(ArmedTool),
    /// Flip the latched weapons-tight stance.
    ToggleWeaponsTight,
}

pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        // Same rule as `UiPlugin`: `command_input` reads `DebugCaptureActive` non-optionally, so this
        // plugin guarantees it. Today the system is skipped headless anyway (its `Single<&Window>` finds
        // no match), which is the only reason the harness never hit the missing-resource panic `UiPlugin`
        // did — that is luck, not a contract, so claim the resource explicitly. `init_resource` is idempotent.
        app.init_resource::<crate::DebugCaptureActive>();
        // Registered here rather than in `UiPlugin` because `arm_tool_input` (the READER) lives here
        // and runs in the harness, where `UiPlugin` never exists. A message whose reader can run
        // without its registration is a panic waiting for the first headless frame.
        app.add_message::<ArmRequest>();
        // Order-issuing input runs in `RunFixedMainLoop` *before* the fixed step, not in `Update`. `Update`
        // runs after the fixed loop, so a `MoveOrder` inserted there wasn't seen by `unit_movement` (on
        // `FixedUpdate`) until the next frame — a one-frame lag. `BeforeFixedMainLoop` flushes the command
        // ahead of the loop's exclusive runner, so the mover consumes it the same frame. It still runs once
        // per frame on fresh `PreUpdate` input, so left-click edge-detection is unaffected.
        app.add_systems(
            RunFixedMainLoop,
            (
                // Guarantee the whole squad is selected before anything reads the selection this frame.
                keep_squad_selected.before(command_input),
                // While a dialogue exchange owns the left-click (choice picks / line advance), don't
                // also issue move orders. `ConversationLock` exists only during a conversation and only
                // in the windowed build (dialogue plugin), so the harness is unaffected — one owner of
                // the click at a time (see `dialogue::runtime`).
                //
                // The same rule now governs the containment verbs: exactly ONE of these five may claim
                // a given left-click, decided by `ArmedTool`. `command_input` keeps the unarmed case,
                // so every input that worked before this change still does exactly what it did.
                arm_tool_input.run_if(not(resource_exists::<ConversationLock>)),
                command_input
                    .run_if(not(resource_exists::<ConversationLock>))
                    .run_if(resource_equals(ArmedTool::None)),
                throw_device_input
                    .run_if(not(resource_exists::<ConversationLock>))
                    .run_if(resource_equals(ArmedTool::Device)),
                place_quarantine_input
                    .run_if(not(resource_exists::<ConversationLock>))
                    .run_if(resource_equals(ArmedTool::Quarantine)),
                cap_nest_input
                    .run_if(not(resource_exists::<ConversationLock>))
                    .run_if(resource_equals(ArmedTool::Cap)),
            )
                .chain()
                // FVS-G-6: none of these mean anything without an expedition, and `command_input` /
                // `place_quarantine_input` take `Res<Dungeon>` non-optionally — the exact pair whose
                // panic blocked a world-less frame. `distributive_run_if`, not `run_if`: the tuple form
                // wraps an anonymous set whose extra graph node permutes the schedule's linearisation
                // and moves the deterministic golden by itself (measured).
                .distributive_run_if(in_state(crate::session::RunState::Active))
                .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
        );
        // Cosmetic only — ring gizmos + cursor icon read state but feed nothing pinned, so they stay on `Update`.
        app.add_systems(Update, (draw_selection_rings, update_cursor).distributive_run_if(in_state(crate::session::RunState::Active)));
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
    capture: Res<crate::DebugCaptureActive>,
    mut sfx: MessageWriter<Sfx>,
) {
    // Stand down while the dev-only region-capture tool (Ctrl+P) owns the mouse, so a capture drag
    // doesn't also issue a squad move order. Always `false` in release (the plugin isn't registered).
    if capture.0 {
        return;
    }
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

/// Arm / disarm a containment verb, and toggle weapons tight.
///
/// **Key choices are constrained, not arbitrary.** `Digit0`–`Digit9` are the time-control rungs,
/// `Q`/`E`/`WASD` drive the camera, `Escape` opens the pause menu, and `H`/`T`/`P`/`Space`/`F3`/`F4`/
/// `F6`/`F10` are all taken. `C`/`Z`/`X` are the free adjacent bottom-row cluster and `F` is free for
/// fire discipline.
///
/// Re-pressing an armed verb disarms it, and so does a right-click — there is no modal state the player
/// can get stuck in, and `Escape` is left alone so it always means "pause".
pub fn arm_tool_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    capture: Res<crate::DebugCaptureActive>,
    mut requests: MessageReader<ArmRequest>,
    mut armed: ResMut<ArmedTool>,
    mut tight: ResMut<crate::laser::WeaponsTight>,
    mut sfx: MessageWriter<Sfx>,
) {
    if capture.0 {
        return;
    }
    // Weapons tight is a latched stance, not a held key: containment holds run for seconds and asking
    // the player to hold a key through one would compete with the mouse.
    let mut toggle_tight = keys.just_pressed(KeyCode::KeyF);

    let mut requested = if keys.just_pressed(KeyCode::KeyC) {
        Some(ArmedTool::Device)
    } else if keys.just_pressed(KeyCode::KeyZ) {
        Some(ArmedTool::Quarantine)
    } else if keys.just_pressed(KeyCode::KeyX) {
        Some(ArmedTool::Cap)
    } else {
        None
    };

    // The clickable verb bar arrives here rather than writing `ArmedTool` itself, so this stays the
    // single writer and the click and the key cannot diverge — clicking a chip *is* pressing its
    // key, including the toggle-to-disarm behaviour and the `DebugCaptureActive` stand-down above.
    for req in requests.read() {
        match *req {
            ArmRequest::Toggle(tool) => requested = Some(tool),
            ArmRequest::ToggleWeaponsTight => toggle_tight = true,
        }
    }

    if toggle_tight {
        tight.0 = !tight.0;
        sfx.write(Sfx::MoveOrder);
    }
    if let Some(want) = requested {
        // Toggle: pressing the armed verb's own key puts it away.
        *armed = if *armed == want { ArmedTool::None } else { want };
        return;
    }
    if mouse.just_pressed(MouseButton::Right) && *armed != ArmedTool::None {
        *armed = ArmedTool::None;
    }
}

/// Throw a capture device at the anomaly under the cursor (archetype 1).
///
/// The device **names** its target rather than searching for one at deploy time — that is FVS-B-5's
/// design and it is what keeps `deploy_devices` free of a pick. The pick happens here instead, once, in
/// windowed input, and it is resolved through `containment::verbs::pick_target` so a tie between two
/// co-located anomalies is broken by `TargetId` rather than by ECS iteration order.
pub fn throw_device_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    capture: Res<crate::DebugCaptureActive>,
    tuning: Res<crate::sim::SimTuning>,
    mut supply: ResMut<DeviceSupply>,
    mut armed: ResMut<ArmedTool>,
    targets: Query<(&TargetId, Entity, &Transform), With<crate::containment::Containment>>,
    units: Query<(&Transform, &crate::health::Health), With<Unit>>,
    mut sfx: MessageWriter<Sfx>,
) {
    if capture.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, cam_tf) = *camera;
    let Some(point) = cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };
    if supply.0 == 0 {
        warn!("no capture devices left this expedition");
        sfx.write(Sfx::Invalid);
        return;
    }
    // Aim tolerance is generous relative to the reach: the player should be able to click a creature,
    // not a pixel. Whether the throw CONNECTS is `deploy_devices`' call, using `reach` below.
    let Some(target) = crate::containment::verbs::pick_target(
        point,
        aim_tolerance(tuning.containment.device_reach),
        targets.iter().map(|(id, e, tf)| (*id, e, tf.translation)),
    )
    else {
        warn!("no containable anomaly under the cursor");
        sfx.write(Sfx::Invalid);
        return;
    };
    // Thrown from the nearest living operative, so "get close enough to throw" is the mechanic. A dead
    // squad cannot throw.
    let Some(from) = units
        .iter()
        .filter(|(_, hp)| hp.current > 0.0)
        .map(|(tf, _)| tf.translation)
        .min_by(|a, b| {
            let (da, db) = ((a.xz() - point.xz()).length(), (b.xz() - point.xz()).length());
            da.total_cmp(&db)
        })
    else {
        return;
    };

    commands.spawn((
        crate::session::run_scoped(),
        crate::containment::ContainmentDevice { target, reach: tuning.containment.device_reach },
        Transform::from_translation(from),
    ));
    // Spent on the throw, not on the connection — a miss costs you the canister. `deploy_devices`
    // already treats a dead/out-of-reach/already-contained target as a spent miss.
    supply.0 -= 1;
    *armed = ArmedTool::None;
    sfx.write(Sfx::MoveOrder);
}

/// Place a quarantine region on the floor under the cursor (archetype 2).
///
/// A ground point, not an entity — so unlike the device throw this is **not a pick** and needs no
/// canonical order.
pub fn place_quarantine_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    dungeon: Res<Dungeon>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    capture: Res<crate::DebugCaptureActive>,
    tuning: Res<crate::sim::SimTuning>,
    mut supply: ResMut<QuarantineSupply>,
    mut armed: ResMut<ArmedTool>,
    mut sfx: MessageWriter<Sfx>,
) {
    if capture.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, cam_tf) = *camera;
    let Some(point) = cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };
    if supply.0 == 0 {
        warn!("no quarantine charges left this expedition");
        sfx.write(Sfx::Invalid);
        return;
    }
    let Some(cell) = nearest_floor(&dungeon, dungeon.world_to_cell(point)) else {
        warn!("quarantine ignored: no floor within {SNAP_MAX_RING} cells of the click");
        sfx.write(Sfx::Invalid);
        return;
    };
    commands.spawn((
        crate::session::run_scoped(),
        crate::containment::Quarantine { radius: tuning.containment.quarantine_radius },
        Transform::from_translation(dungeon.cell_center(cell)),
    ));
    supply.0 -= 1;
    *armed = ArmedTool::None;
    sfx.write(Sfx::MoveOrder);
}

/// Cap the nest under the cursor (archetype 3).
///
/// **Grants nothing, on purpose.** `Capped` is a terminal marker with no `on_add` hook: sealing a
/// structure is honestly "kill the source for no specimen", and giving it a reward would quietly undo
/// the win-by-containing pivot the whole backlog is built on. Same pick discipline as the device.
pub fn cap_nest_input(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    capture: Res<crate::DebugCaptureActive>,
    tuning: Res<crate::sim::SimTuning>,
    mut armed: ResMut<ArmedTool>,
    nests: Query<
        (&TargetId, Entity, &Transform),
        (With<crate::nest::Nest>, Without<crate::containment::Capped>),
    >,
    units: Query<(&Transform, &crate::health::Health), With<Unit>>,
    mut sfx: MessageWriter<Sfx>,
) {
    if capture.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, cam_tf) = *camera;
    let Some(point) = cursor_ground_point(&window, camera, cam_tf) else {
        return;
    };
    let Some((nest, nest_pos)) = crate::containment::verbs::pick_target(
        point,
        aim_tolerance(tuning.containment.cap_reach),
        nests.iter().map(|(id, e, tf)| (*id, (e, tf.translation), tf.translation)),
    ) else {
        warn!("no uncapped nest under the cursor");
        sfx.write(Sfx::Invalid);
        return;
    };
    // Reach check mirrors `device::deploy_devices`: a unit has to be at the nest to seal it.
    let in_reach = units.iter().any(|(tf, hp)| {
        hp.current > 0.0
            && (tf.translation.xz() - nest_pos.xz()).length() <= tuning.containment.cap_reach
    });
    if !in_reach {
        warn!("no operative within {} m of that nest", tuning.containment.cap_reach);
        sfx.write(Sfx::Invalid);
        return;
    }
    commands.entity(nest).insert(crate::containment::Capped);
    *armed = ArmedTool::None;
    sfx.write(Sfx::MoveOrder);
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
        gizmos.circle(iso, RING_RADIUS, crate::palette::SELECTION_RING);
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

#[cfg(test)]
mod aim_tests {
    use super::*;

    #[test]
    fn the_affordance_is_never_tighter_than_the_mechanic() {
        // THE invariant `AIM_TOLERANCE`'s doc states and the shipped constants violated: aim is a UI
        // affordance ("did you mean that one?"), reach is the mechanic ("are you close enough?"), and
        // an aim tighter than the reach means a player who IS in range is told they missed. Nine
        // consecutive failed cap attempts in a play log is what that looks like from the outside.
        for reach in [1.5_f32, 2.5, 3.0] {
            assert!(
                aim_tolerance(reach) >= reach,
                "aim {} must not be tighter than reach {reach}",
                aim_tolerance(reach)
            );
        }
    }

    #[test]
    fn a_short_reach_still_gets_the_baseline_affordance() {
        // The `max` floor: a verb with a tiny reach must not inherit a tiny click target. Clicking is a
        // mouse-precision problem, not a game-distance one, so it has its own minimum.
        assert_eq!(aim_tolerance(0.1), AIM_TOLERANCE);
        assert!(aim_tolerance(0.0) > 0.0);
    }

    #[test]
    fn the_shipped_reaches_are_all_looser_than_they_were() {
        // Pins the actual regression: every shipped verb reach exceeds the old flat constant, so all
        // three were mis-gated by the aim, not by the mechanic.
        for reach in [1.5_f32, 2.5, 3.0] {
            assert!(reach > AIM_TOLERANCE, "reach {reach} was tighter-gated by aim {AIM_TOLERANCE}");
        }
    }
}
