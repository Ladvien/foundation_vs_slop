//! **What the editor draws on the world** — footprints, the selection, the drag ghost, and a mark on
//! every prop breaking a rule.
//!
//! All of it is `Gizmos`, following `containment::cordon`: no entities, nothing to keep in step, and
//! nothing that can outlive the frame it was drawn for. `Gizmos` is not registered in the headless
//! harness at all, so none of this can reach a test that measures the sim.
//!
//! # The ghost sits where the prop will land
//!
//! `cordon::preview_cordon` states the rule this follows: *"a preview drawn at the cursor would sit
//! somewhere the cordon will not be — which is worse than no preview, because it is a promise the game
//! then breaks."* So the drag ghost is drawn at the **snapped** position, and it is the same rectangle
//! the placement rules will measure — [`pick::footprint`] is the function both use.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use crate::site::kit::SiteKit;
use crate::site::layout::SiteLayout;
use crate::site::pieces::SitePiece;
use crate::ui::theme::UiTheme;

/// Height above the floor to draw at, so an outline never z-fights the floor plate it describes.
/// Same trick and roughly the same value as `cordon::RING_LIFT`.
const OUTLINE_LIFT: f32 = 0.05;

/// Draw one prop footprint as a world-space rectangle.
///
/// The rectangle is the prop's *native* footprint turned by its yaw — not the axis-aligned box the
/// overlap rule reduces it to — because this is the thing the author is aiming, and showing them a
/// box that grows and shrinks as they rotate would be describing the approximation rather than the
/// prop.
pub fn outline(
    gizmos: &mut Gizmos,
    layout: &SiteLayout,
    kit: &SiteKit,
    piece: SitePiece,
    pos: (f32, f32),
    yaw_deg: f32,
    color: Color,
) {
    let (fw, fd) = kit.footprint(piece);
    let at = layout.point(pos) + Vec3::Y * OUTLINE_LIFT;
    // `Rectangle` is authored in the XY plane; tip it into XZ, then apply the prop's own yaw.
    let rot = Quat::from_rotation_y(yaw_deg.to_radians()) * Quat::from_rotation_x(-FRAC_PI_2);
    gizmos.rect(Isometry3d::new(at, rot), Vec2::new(fw, fd), color);
}

/// A short spur from the prop's centre out through its front face.
///
/// Two of the six placement rules are about *facing* — a seat must address its surface, and nothing
/// fronted may face a wall — and yaw is the one property of a prop you cannot read off a rectangle.
/// `KitPiece::front` is the mesh's own quarter-turn offset from the engine convention, and applying it
/// here is what makes this line agree with the rules rather than being 90° wrong for every chair.
pub fn facing(
    gizmos: &mut Gizmos,
    layout: &SiteLayout,
    kit: &SiteKit,
    piece: SitePiece,
    pos: (f32, f32),
    yaw_deg: f32,
    color: Color,
) {
    let Some(front) = kit.front(piece) else {
        return;
    };
    let (fw, fd) = kit.footprint(piece);
    let reach = 0.5 * fw.max(fd) + 0.35;
    let yaw = (yaw_deg + front).to_radians();
    let from = layout.point(pos) + Vec3::Y * OUTLINE_LIFT;
    let to = from + Vec3::new(yaw.sin() * reach, 0.0, yaw.cos() * reach);
    gizmos.line(from, to, color);
}

/// Everything the editor draws in one pass, in back-to-front importance order so the most urgent mark
/// is the one left on top.
pub fn draw(
    gizmos: &mut Gizmos,
    theme: &UiTheme,
    layout: &SiteLayout,
    kit: &SiteKit,
    faults: &[crate::site::layout::PlacementFault],
    selected: Option<usize>,
    hovered: Option<usize>,
    ghost: Option<(SitePiece, (f32, f32), f32)>,
) {
    // Every prop, faintly — the author needs to see what is claimed before they can see what collides.
    for p in &layout.props {
        outline(
            gizmos,
            layout,
            kit,
            p.piece,
            p.pos,
            p.yaw,
            theme.text_muted.with_alpha(0.35),
        );
    }

    // Anything breaking a rule, including the second prop of an overlapping pair — either one moving
    // would resolve it, so marking only the first would point at an arbitrary half of the problem.
    for f in faults {
        for ix in [Some(f.prop), f.other].into_iter().flatten() {
            if let Some(p) = layout.props.get(ix) {
                outline(gizmos, layout, kit, p.piece, p.pos, p.yaw, theme.danger);
                facing(gizmos, layout, kit, p.piece, p.pos, p.yaw, theme.danger);
            }
        }
    }

    if let Some(p) = hovered.and_then(|ix| layout.props.get(ix)) {
        outline(gizmos, layout, kit, p.piece, p.pos, p.yaw, theme.text);
    }

    if let Some(p) = selected.and_then(|ix| layout.props.get(ix)) {
        outline(gizmos, layout, kit, p.piece, p.pos, p.yaw, theme.accent);
        facing(gizmos, layout, kit, p.piece, p.pos, p.yaw, theme.accent);
    }

    // The promise: where the thing will actually be when the button comes up.
    if let Some((piece, pos, yaw)) = ghost {
        outline(gizmos, layout, kit, piece, pos, yaw, theme.accent);
        facing(gizmos, layout, kit, piece, pos, yaw, theme.accent);
    }
}
