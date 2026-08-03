//! **The placement ghost** — a see-through copy of the armed piece, standing where a click would put it.
//!
//! The wireframe footprint in [`super::overlay`] answers "how much floor does this claim". It does not
//! answer "what is this, and which way is it facing", and at the zoom an author actually works at a
//! 0.6 m crate is a handful of pixels — easy to place and not notice. So the armed brush gets a real
//! model, at the snapped position, semi-transparent.
//!
//! It follows `containment::cordon`'s rule about previews: drawn at the **snapped** cell, never at the
//! raw cursor, because *"a preview drawn at the cursor would sit somewhere the cordon will not be —
//! which is worse than no preview, because it is a promise the game then breaks."*
//!
//! # Why the materials are cloned
//!
//! The mesh comes from the kit's GLB, so its materials are the *shared* handles every real instance of
//! that piece uses. Writing alpha into them would turn every crate in the hub see-through. Each ghost
//! descendant therefore gets its own cloned `StandardMaterial` with `AlphaMode::Blend`, applied once
//! and marked with [`Ghosted`] so the walk does not repeat.
//!
//! Cloning happens in a second system rather than at spawn because a GLB scene instantiates over
//! several frames — there are no material handles to clone on the frame the ghost is created.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::site::kit::SiteKit;
use crate::site::pieces::SitePiece;
use crate::site::SiteKitRes;
use crate::site::visuals::SiteLayoutRes;

use super::{pick, Drag, EditorState};

/// How much of the original opacity a ghost keeps.
const GHOST_ALPHA: f32 = 0.45;

/// Root of the placement ghost.
#[derive(Component)]
pub struct Ghost;

/// Which piece the live ghost is showing, so it is only rebuilt when the brush actually changes —
/// respawning a GLB every frame would thrash the asset server and never finish loading.
#[derive(Component)]
pub struct GhostPiece(pub SitePiece);

/// Marks a descendant whose material has already been cloned and faded.
#[derive(Component)]
pub struct Ghosted;

/// Keep one ghost alive, showing the armed brush at the snapped cursor.
#[allow(clippy::too_many_arguments)]
pub fn drive_ghost(
    mut commands: Commands,
    state: Res<EditorState>,
    kit: Option<Res<SiteKitRes>>,
    layout: Option<Res<SiteLayoutRes>>,
    assets: Res<AssetServer>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), Without<crate::ThumbnailCamera>>>,
    ghosts: Query<(Entity, &GhostPiece), With<Ghost>>,
    mut transforms: Query<&mut Transform, With<Ghost>>,
) {
    let clear = |commands: &mut Commands, ghosts: &Query<(Entity, &GhostPiece), With<Ghost>>| {
        for (e, _) in ghosts {
            commands.entity(e).despawn();
        }
    };

    // No ghost while dragging an existing prop — that interaction has its own preview, and two
    // promises about where something is going is one too many.
    if !state.open || !matches!(state.drag, Drag::Idle) {
        clear(&mut commands, &ghosts);
        return;
    }
    let (Some(kit), Some(layout), Some(window), Some(camera)) = (kit, layout, window, camera) else {
        clear(&mut commands, &ghosts);
        return;
    };
    let (cam, cam_tf) = *camera;
    let Some(at) = pick::cursor_layout_point(&layout.0, &window, cam, cam_tf) else {
        // Cursor off-window: no honest place to stand.
        clear(&mut commands, &ghosts);
        return;
    };

    let piece = state.brush;
    let pos = pick::snap(at);
    let yaw = state.brush_yaw;

    // Rebuild only when the brush changes.
    let existing = ghosts.iter().find(|(_, p)| p.0 == piece).map(|(e, _)| e);
    for (e, p) in &ghosts {
        if p.0 != piece {
            commands.entity(e).despawn();
        }
    }

    let at_world = ghost_translation(&layout.0, &kit.0, piece, pos);
    match existing {
        Some(e) => {
            if let Ok(mut tf) = transforms.get_mut(e) {
                tf.translation = at_world;
                tf.rotation = Quat::from_rotation_y(yaw.to_radians());
            }
        }
        None => {
            let e = crate::site::visuals::place(&mut commands, &assets, &kit.0, piece, at_world, yaw);
            commands.entity(e).insert((Ghost, GhostPiece(piece)));
        }
    }
}

/// Where the ghost's origin goes — the same arithmetic `site::visuals` uses for a real prop, minus the
/// `resting_on` lift, because a ghost has no host until it is actually placed.
fn ghost_translation(
    layout: &crate::site::layout::SiteLayout,
    kit: &SiteKit,
    piece: SitePiece,
    pos: (f32, f32),
) -> Vec3 {
    layout.point(pos) + Vec3::Y * kit.y_offset(piece)
}

/// Fade the ghost's materials once its GLB has instantiated them.
///
/// Walks only un-[`Ghosted`] descendants, so this settles to a no-op a frame or two after each new
/// ghost appears rather than re-cloning materials forever.
pub fn fade_ghost(
    mut commands: Commands,
    ghosts: Query<Entity, With<Ghost>>,
    children: Query<&Children>,
    painted: Query<&MeshMaterial3d<StandardMaterial>, Without<Ghosted>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for root in &ghosts {
        let mut queue = vec![root];
        while let Some(e) = queue.pop() {
            if let Ok(handle) = painted.get(e) {
                if let Some(base) = materials.get(&handle.0) {
                    let mut faded = base.clone();
                    // Blend, not Mask: the point is to see the room through it.
                    faded.alpha_mode = AlphaMode::Blend;
                    let a = faded.base_color.alpha() * GHOST_ALPHA;
                    faded.base_color.set_alpha(a);
                    // A ghost is a diagram, not a prop — it should not cast shadows into the room it
                    // is not in yet.
                    faded.unlit = false;
                    let handle = materials.add(faded);
                    commands
                        .entity(e)
                        .insert((MeshMaterial3d(handle), Ghosted, NotShadowCaster));
                }
            }
            if let Ok(kids) = children.get(e) {
                queue.extend(kids.iter());
            }
        }
    }
}
