//! **Precision mode** — Bevy 0.19's first-party transform gizmo driving the selected prop.
//!
//! Ground drag is the editor's default verb because it matches what a floor plan *is*: you point at
//! where the thing goes. But "nudge this console 0.5 m along Z without touching X" is genuinely
//! awkward with a free drag under an isometric camera, where both ground axes run diagonally across
//! the screen. `G` switches to axis handles for exactly that.
//!
//! **This is one edit path with two input devices, not two paths to one result.** Both modes end in
//! the same [`EditorDoc::move_prop`] call, which is the only thing in the module that can change a
//! record. What differs is how the author names a position.
//!
//! # Translate and Rotate only — never Scale
//!
//! `site::visuals::place` bakes `kit.scale(piece) * kit.y_scale(piece)` into `Transform.scale`: the
//! uniform art correction that brings a mesh authored at the wrong size down to life-size, times the
//! policy stretch that makes a wall reach `WALL_HEIGHT`. And `site::cutaway` writes
//! `Transform.scale.y` on Site walls every frame to squash the ones facing the camera.
//!
//! A scale drag would fight both, and the damage would be invisible in the file — `PropPlacement` has
//! no scale field, so there is nowhere for the change to be written and nothing to diff. It would
//! simply look like the kit had changed until the next relaunch. So [`TransformGizmoMode::Scale`] is
//! never set, and the readback below takes translation and yaw and nothing else.
//!
//! # Why the readback watches a state edge
//!
//! The gizmo writes `Transform` directly, every frame of a drag. Committing each frame would fill the
//! undo stack with one entry per frame, so the record is written once, when
//! `TransformGizmoState::active` falls — which is the same "commit on release" shape ground drag uses.

use bevy::gizmos::transform_gizmo::{
    TransformGizmoFocus, TransformGizmoMode, TransformGizmoSettings, TransformGizmoState,
};
use bevy::prelude::*;

use crate::site::visuals::PropIndex;
use crate::site::SiteKitRes;

use super::{pick, EditorState};

/// Translation snap, metres — the same half-metre grid ground drag uses, so the two modes cannot
/// disagree about where a legal position is.
const SNAP_TRANSLATE: f32 = 0.5;

/// Rotation snap, radians. [`pick::YAW_STEP_DEG`] in the unit the gizmo wants.
const SNAP_ROTATE: f32 = std::f32::consts::PI / 12.0;

/// Configure the gizmo. Called from the plugin's `build`.
pub fn settings() -> TransformGizmoSettings {
    TransformGizmoSettings {
        mode: TransformGizmoMode::Translate,
        snap_translate: Some(SNAP_TRANSLATE),
        snap_rotate: Some(SNAP_ROTATE),
        // Never `Some(..)`: see the module header. Scale is not a property of a `PropPlacement`.
        snap_scale: None,
        // The cursor must stay free — the editor's palette is on screen and the author needs to
        // reach it between drags.
        confine_cursor: false,
        ..default()
    }
}

/// Put the gizmo on the selected prop while precision mode is on, and take it off otherwise.
///
/// `TransformGizmoFocus` is a marker the gizmo looks for, so adding and removing it is how the tool
/// says "manipulate this one". Removing it on mode-off is what stops handles hanging over a prop that
/// ground drag is about to move.
pub fn track_focus(
    mut commands: Commands,
    state: Res<EditorState>,
    focused: Query<Entity, With<TransformGizmoFocus>>,
    props: Query<(Entity, &PropIndex)>,
) {
    let want = state
        .gizmo_mode
        .then_some(state.selected)
        .flatten()
        .and_then(|ix| props.iter().find(|(_, p)| p.0 == ix).map(|(e, _)| e));

    for e in &focused {
        if Some(e) != want {
            commands.entity(e).remove::<TransformGizmoFocus>();
        }
    }
    if let Some(e) = want {
        if !focused.contains(e) {
            commands.entity(e).insert(TransformGizmoFocus);
        }
    }
}

/// Switch the gizmo between Translate and Rotate with `R`, and keep Scale unreachable.
pub fn cycle_mode(keys: Res<ButtonInput<KeyCode>>, mut settings: ResMut<TransformGizmoSettings>) {
    if !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    settings.mode = match settings.mode {
        TransformGizmoMode::Translate => TransformGizmoMode::Rotate,
        // Rotate *and* the unreachable Scale both fall back to Translate, so there is no key
        // sequence that can land on Scale.
        _ => TransformGizmoMode::Translate,
    };
}

/// When a gizmo drag ends, write the entity's new pose back into the record.
///
/// Reads translation and yaw only. The Y translation is deliberately dropped: a prop's height is
/// *derived* — floor level, plus `kit.y_offset`, plus the host's top for anything that rests on a
/// surface — so there is no field to write it to and inventing one would make the file disagree with
/// `layout::resting_on`.
pub fn commit_on_release(
    mut state: ResMut<EditorState>,
    gizmo: Res<TransformGizmoState>,
    mut was_active: Local<bool>,
    kit: Option<Res<SiteKitRes>>,
    props: Query<(&PropIndex, &Transform)>,
) {
    let active = gizmo.active;
    let released = *was_active && !active;
    *was_active = active;
    if !released {
        return;
    }

    let (Some(kit), Some(entity)) = (kit, gizmo.entity) else {
        return;
    };
    let Ok((ix, tf)) = props.get(entity) else {
        return;
    };
    let Some(doc) = state.doc.as_ref() else { return };
    let origin = doc.layout.origin;

    let pos = pick::snap((tf.translation.x - origin.0, tf.translation.z - origin.2));
    let yaw = pick::snap_yaw(tf.rotation.to_euler(EulerRot::YXZ).0.to_degrees());
    let index = ix.0;

    super::commit(&mut state, &kit.0, |doc, kit| {
        doc.move_prop(index, pos, yaw, kit)
    });
    state.status = format!("#{index} moved to {pos:?} yaw {yaw}");
}
