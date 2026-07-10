//! **Squad-AI debug overlay** — a keybind-toggled label over each unit showing the state its brain is
//! actually in.
//!
//! Until now the only way to inspect squad AI was to flip the `AI_DIAG` compile-time constant in
//! `ai::diag` and rebuild, then read a 1 Hz console histogram. That is a poor instrument for the class of
//! bug this module exists to expose: **mode thrash**, where a unit flips between two behaviours on
//! consecutive thinks. Thrash is obvious in one second of watching a label and nearly invisible in a log.
//!
//! Distinct from `psi_vision`, which is a game mechanic. This is a developer tool: press `F3`.
//!
//! Cosmetic → `Update`, reusing the dialogue bubble's rasteriser and billboard tracker rather than growing
//! a second one. Nothing here is read by the pinned sim, so it cannot enter `snapshot_hash`.

use bevy::prelude::*;

use crate::ai::brain::ActiveBehavior;
use crate::ai::drives::{DriveId, Drives};
use crate::dialogue::bubble::{build_bubble, Bubble, BubbleAssets, BubbleStyle};
use crate::dialogue::{BubbleKind, Emotion};
use crate::squad::{SquadMember, Unit};
use crate::squad_ai::cohesion::SquadAnchor;
use crate::squad_ai::role::RoleId;

/// Lift the label clear of any dialogue bubble on the same unit.
const LABEL_OFFSET_Y: f32 = 0.55;
/// Extra lift per squad member. A five-unit squad stands in a tight blob (ORCA spaces them by about a
/// body width), so labels anchored at one height overlap into an unreadable stack. Fanning them by member
/// index keeps all five legible — which is the entire point of the tool.
const LABEL_STAGGER_Y: f32 = 0.60;

pub struct AiOverlayPlugin;

impl Plugin for AiOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AiOverlay>()
            .add_systems(Update, (toggle_overlay, sync_labels).chain());
    }
}

/// Whether the debug overlay is drawn. Off by default; `F3` toggles.
#[derive(Resource, Default)]
pub struct AiOverlay {
    pub on: bool,
}

/// A live label, remembering the text it was rasterised with so the material is rebuilt only when the
/// text actually changes — not once per frame, which would mint an orphaned image + material every frame
/// (the leak `squad::recolor_units` shipped once already).
///
/// The owner lives on the co-located [`Bubble`], which is also what billboards this quad and reaps it when
/// the unit dies; duplicating the entity here would be a second source of truth for the same fact.
#[derive(Component)]
struct AiLabel {
    text: String,
}

fn toggle_overlay(keys: Res<ButtonInput<KeyCode>>, mut overlay: ResMut<AiOverlay>) {
    if keys.just_pressed(KeyCode::F3) {
        overlay.on = !overlay.on;
        info!("squad AI overlay: {}", if overlay.on { "on" } else { "off" });
    }
}

/// The one-line state summary for a unit: what it decided, how frightened it is, how far it has strayed.
///
/// These three explain almost every "why is it doing that?" between them — the chosen `Mode` is the
/// decision, FEAR is the drive that outranks every other (`Flee` is rank 6), and the anchor distance is
/// what gates the cohesion leash.
fn label_text(role: RoleId, mode: crate::ai::utility::Mode, fear: f32, anchor_dist: f32) -> String {
    format!("{role:?} {mode:?}\nfear {fear:.1}  anchor {anchor_dist:.1}")
}

#[allow(clippy::too_many_arguments)]
fn sync_labels(
    mut commands: Commands,
    overlay: Res<AiOverlay>,
    assets: Option<Res<BubbleAssets>>,
    anchor: Option<Res<SquadAnchor>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    units: Query<(Entity, &Transform, &RoleId, &ActiveBehavior, &Drives, &SquadMember), With<Unit>>,
    mut labels: Query<
        (Entity, &Bubble, &mut AiLabel, &mut MeshMaterial3d<StandardMaterial>, &mut Transform),
        Without<Unit>,
    >,
) {
    let (Some(assets), Some(anchor)) = (assets, anchor) else {
        return;
    };

    if !overlay.on {
        for (label_entity, _, _, _, _) in &labels {
            commands.entity(label_entity).despawn();
        }
        return;
    }

    let describe = |unit: Entity| -> Option<String> {
        let (_, tf, role, active, drives, _) = units.get(unit).ok()?;
        let dist = if anchor.valid {
            (anchor.pos - tf.translation).length()
        } else {
            crate::ai::utility::NO_TARGET_DIST
        };
        Some(label_text(*role, active.mode, drives.get(DriveId::FEAR), dist))
    };

    // Refresh the labels that exist, noting which units they cover. A label whose owner died is left for
    // `track_bubbles` to reap — it already owns that lifecycle for every `Bubble`.
    let mut covered: Vec<Entity> = Vec::new();
    for (_, bubble, mut label, mut material, mut tf) in &mut labels {
        let Some(want) = describe(bubble.owner) else { continue };
        covered.push(bubble.owner);
        if want == label.text {
            continue;
        }
        let rendered = build_bubble(&assets, &mut images, &mut materials, &style(), &want);
        material.0 = rendered.material;
        tf.scale = Vec3::new(rendered.size.x, rendered.size.y, 1.0);
        label.text = want;
    }

    // ...and mint one for any unit that doesn't have a label yet (the first frame after the toggle).
    for (unit, _, _, _, _, member) in &units {
        if covered.contains(&unit) {
            continue;
        }
        let Some(text) = describe(unit) else { continue };
        let rendered = build_bubble(&assets, &mut images, &mut materials, &style(), &text);
        let lift = LABEL_OFFSET_Y + member.0 as f32 * LABEL_STAGGER_Y;
        commands.spawn((
            Bubble { owner: unit, offset: Vec2::new(0.0, lift) },
            AiLabel { text },
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(rendered.material),
            Transform::from_scale(Vec3::new(rendered.size.x, rendered.size.y, 1.0)),
        ));
    }
}

/// A plain, tail-less plate — this is instrumentation, not something a character is saying.
fn style() -> BubbleStyle {
    BubbleStyle { kind: BubbleKind::Thought, emotion: Emotion::Neutral, tail: false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::utility::Mode;

    #[test]
    fn the_label_names_the_decision_the_drive_and_the_leash() {
        // The overlay exists to answer "why is it doing that?", so all three inputs must be legible.
        let s = label_text(RoleId::Gunman, Mode::Overwatch, 0.42, 3.14);
        assert!(s.contains("Gunman") && s.contains("Overwatch"), "{s}");
        assert!(s.contains("fear 0.4"), "{s}");
        assert!(s.contains("anchor 3.1"), "{s}");
    }

    #[test]
    fn the_label_is_stable_between_identical_states() {
        // `sync_labels` rebuilds the texture only when this string changes, so tiny float jitter must not
        // produce a new string every frame — that was a per-frame orphaned-material leak once before.
        let a = label_text(RoleId::Medic, Mode::TendWounded, 0.2000, 5.0400);
        let b = label_text(RoleId::Medic, Mode::TendWounded, 0.2004, 5.0401);
        assert_eq!(a, b, "sub-precision jitter must not force a texture rebuild");
    }
}
