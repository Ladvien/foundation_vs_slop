//! **Off-screen indicators** — where the extraction point is, and where the squad went.
//!
//! # The two ways this camera loses things
//!
//! `camera` is a free-panning RTS rig that **follows nothing**. That is the right choice for a game
//! commanded by mouse, and it has two consequences the game did not handle:
//!
//! 1. The extraction point is a fixed world position the objective line names by *instruction*
//!    (`RETURN TO THE EXTRACTION POINT`) with no way to find it. `containment::extraction`'s beacon
//!    fixes that once you are looking the right way; this fixes knowing which way that is.
//! 2. Since selection became real (`crate::selection`), the player can order three operatives away
//!    and keep two — so "the squad" is no longer one blob that the camera happens to be near.
//!
//! # Why an edge blob and not an icon
//!
//! Rosenholtz 2016 (*Capabilities and Limitations of Peripheral Vision*, Annu. Rev. Vis. Sci. 2,
//! DOI 10.1146/annurev-vision-082114-035733): peripheral vision is not low-resolution foveal vision,
//! it is a **lossy summary-statistic encoding**. An element parked at the screen edge is read as
//! texture statistics — a colour blob, gross shape, motion — so icon detail and small text out there
//! are wasted pixels. These markers are therefore a filled triangle-ish block plus a slow pulse, and
//! nothing else.
//!
//! The pulse is deliberately **slow and low-contrast**, not a flash. Lewandowska, Dziśko & Jankowski
//! 2022 (`10.1038/s41598-022-16284-2`, already the source for `docs/ui.md` §1.3's alert rule) found a
//! **medium** contrast level and ~2 Hz sufficient for peripheral visibility, and that sustained high
//! intensity "can cause unnecessary irritation or even cognitive load for more extended usage". A
//! permanent marker is the extended-usage case by definition, so it sits at the bottom of that band.
//!
//! # Why these are absolutely positioned, unlike every other panel
//!
//! `ui::layout`'s nine regions exist so panels cannot fight over corners. A marker that tracks a
//! *world point* has no corner to be assigned — its position is the output of a projection, and
//! clamping it into a third of the screen would be a lie about where the thing is. This is the
//! legitimate case for `PositionType::Absolute`; `knowledge::roster`'s overlay is not (it has no
//! world anchor and simply predates the grid).
//!
//! Windowed-only, `Update` only, reads sim state and writes none of it — so nothing here can reach
//! `snapshot_hash`.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::layout::HudRegions;
use super::state::{despawn_scoped, AppState};
use super::theme::{UiTheme, Z_HUD};
use crate::containment::ExtractionZone;
use crate::squad::{Selected, Unit};

/// Root marker so the whole set tears down with the screen.
#[derive(Component)]
pub struct OffscreenRoot;

/// What a marker is pointing at. One entity per kind, spawned once and shown/hidden — cheaper than
/// spawning per frame, and it keeps the node count fixed so a leak is impossible.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkerKind {
    /// The way out. Always tracked while a zone exists.
    Extraction,
    /// The centroid of the *selected* operatives — the ones the player's next order will move.
    Selection,
}

impl MarkerKind {
    pub const ALL: [MarkerKind; 2] = [MarkerKind::Extraction, MarkerKind::Selection];
}

/// Distance in logical px from the window edge at which a marker rides.
///
/// Inset rather than flush so the marker is not half-clipped, and so it does not collide with the
/// panels the region grid puts in the corners.
const EDGE_INSET: f32 = 26.0;

/// Marker size in logical px. Small: it is a summary statistic, not a readout.
const MARKER: f32 = 14.0;

/// Pulse rate in Hz. The bottom of Lewandowska et al.'s effective band, because this marker is
/// on-screen for the whole run.
const PULSE_HZ: f32 = 1.0;

pub struct OffscreenIndicatorPlugin;

impl Plugin for OffscreenIndicatorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::InGame),
            spawn_markers.after(super::layout::spawn_frame),
        )
        .add_systems(OnExit(AppState::InGame), despawn_scoped::<OffscreenRoot>)
        .add_systems(
            Update,
            update_markers
                .run_if(in_state(AppState::InGame))
                .run_if(in_state(crate::session::RunState::Active)),
        );
    }
}

fn spawn_markers(mut commands: Commands, theme: Res<UiTheme>, regions: Res<HudRegions>) {
    // Parented to the frame's root region only so it tears down with the HUD; the node itself is
    // absolutely positioned against the window (see the module note).
    if regions.get(super::layout::Region::TopLeft).is_none() {
        error!("offscreen markers: no layout frame at spawn — the extraction bearing is not shown");
        return;
    }
    for kind in MarkerKind::ALL {
        commands.spawn((
            OffscreenRoot,
            kind,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(MARKER),
                height: Val::Px(MARKER),
                // A field on `Node`, NOT a component — `docs/ui.md` §5 names this exact trap
                // (it changed in Bevy 0.18). Spelled as a bundle element it does not compile.
                border_radius: BorderRadius::all(Val::Px(MARKER * 0.5)),
                // Hidden until `update_markers` decides the target is off-screen.
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme.accent),
            GlobalZIndex(Z_HUD),
            Pickable::IGNORE,
        ));
    }
}

/// Where a marker should sit, and how bright, for one target.
///
/// Pure so the geometry is testable without a camera or a window — the projection is the caller's
/// job, this decides what to do with the result.
///
/// `screen` is the target's viewport position (which may be outside `size`, or `None` when the
/// projection failed). Returns `None` when the target is comfortably on-screen and needs no marker:
/// a marker for something the player can already see is pure noise, and `docs/ui.md` §1.2 is explicit
/// that noise is not neutral.
fn marker_placement(screen: Option<Vec2>, size: Vec2) -> Option<Vec2> {
    let inset = EDGE_INSET;
    // A target whose projection failed is behind the camera or otherwise unresolvable. With a fixed
    // top-down-ish iso rig that is nearly unreachable, but "nearly" is not "never", and the honest
    // answer is still "not visible" — so mark it, parked at the screen centre-bottom rather than
    // guessed at. Anything else would be inventing a bearing.
    let Some(p) = screen else {
        return Some(Vec2::new(size.x * 0.5, size.y - inset));
    };
    let on_screen = p.x >= inset && p.x <= size.x - inset && p.y >= inset && p.y <= size.y - inset;
    if on_screen {
        return None;
    }
    // Clamp into the inset frame. The clamped point IS the bearing: the marker sits on the edge
    // nearest the target, which is what a player reads as "that way".
    Some(Vec2::new(
        p.x.clamp(inset, (size.x - inset).max(inset)),
        p.y.clamp(inset, (size.y - inset).max(inset)),
    ))
}

/// Slow pulse alpha, in the medium-contrast band.
fn pulse_alpha(elapsed: f32) -> f32 {
    // 0.45..0.85 — visible in the periphery, never a flash. `reduce_flashing` is not consulted
    // because this never reaches the high-contrast burst regime that setting exists to damp.
    let t = (elapsed * PULSE_HZ * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    0.45 + 0.40 * t
}

#[allow(clippy::too_many_arguments)]
fn update_markers(
    time: Res<Time<Real>>,
    theme: Res<UiTheme>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    zones: Query<(&ExtractionZone, &Transform)>,
    selected: Query<&Transform, (With<Unit>, With<Selected>)>,
    mut markers: Query<(&MarkerKind, &mut Node, &mut BackgroundColor)>,
) {
    let (camera, cam_tf) = *camera;
    let size = window.size();
    let alpha = pulse_alpha(time.elapsed_secs());

    // The selection's centroid. Mean of a SET, so no ordering decision is made and no total sort is
    // needed — addition is the only operation and every member contributes exactly once.
    let mut sum = Vec3::ZERO;
    let mut n = 0u32;
    for tf in &selected {
        sum += tf.translation;
        n += 1;
    }
    let selection_centre = (n > 0).then(|| sum / n as f32);
    let extraction_centre = zones.iter().next().map(|(_, tf)| tf.translation);

    for (kind, mut node, mut bg) in &mut markers {
        let world = match kind {
            MarkerKind::Extraction => extraction_centre,
            MarkerKind::Selection => selection_centre,
        };
        // No target is a real state (no zone this run; nothing selected) and the honest response is
        // to show nothing, not to park a marker at the origin.
        let Some(world) = world else {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        let screen = camera.world_to_viewport(cam_tf, world).ok();
        match marker_placement(screen, size) {
            Some(at) => {
                node.display = Display::Flex;
                node.left = Val::Px(at.x - MARKER * 0.5);
                node.top = Val::Px(at.y - MARKER * 0.5);
                // Both markers are the same warm neutral as the rest of the HUD; they are told apart
                // by *what they do* (one is static, one tracks your operatives), not by hue — the
                // §1.3 encoding rule. The selection marker is the dimmer of the two because losing
                // the way out matters more than losing sight of a unit you just ordered.
                let base = match kind {
                    MarkerKind::Extraction => theme.accent,
                    MarkerKind::Selection => theme.text_muted,
                };
                bg.0 = base.with_alpha(alpha);
            }
            None => {
                if node.display != Display::None {
                    node.display = Display::None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: Vec2 = Vec2::new(1280.0, 720.0);

    #[test]
    fn a_visible_target_gets_no_marker() {
        // `docs/ui.md` §1.2: a widget that supports no decision is noise, and noise is not neutral.
        // A marker pointing at something already on screen is exactly that.
        assert_eq!(marker_placement(Some(Vec2::new(640.0, 360.0)), SIZE), None);
    }

    #[test]
    fn a_target_past_each_edge_is_clamped_to_that_edge() {
        // The clamped position IS the bearing, so it must land on the side the target is actually on.
        let left = marker_placement(Some(Vec2::new(-500.0, 360.0)), SIZE).expect("off-screen");
        assert_eq!(left.x, EDGE_INSET, "a target to the left rides the left edge");

        let right = marker_placement(Some(Vec2::new(9000.0, 360.0)), SIZE).expect("off-screen");
        assert_eq!(right.x, SIZE.x - EDGE_INSET);

        let up = marker_placement(Some(Vec2::new(640.0, -80.0)), SIZE).expect("off-screen");
        assert_eq!(up.y, EDGE_INSET);

        let down = marker_placement(Some(Vec2::new(640.0, 5000.0)), SIZE).expect("off-screen");
        assert_eq!(down.y, SIZE.y - EDGE_INSET);
    }

    #[test]
    fn a_corner_target_keeps_both_bearings() {
        // Clamping each axis independently is what preserves "up AND to the left" — collapsing to the
        // nearest single edge would throw away half the information the marker exists to carry.
        let at = marker_placement(Some(Vec2::new(-400.0, -400.0)), SIZE).expect("off-screen");
        assert_eq!(at, Vec2::new(EDGE_INSET, EDGE_INSET));
    }

    #[test]
    fn a_target_just_inside_the_inset_is_treated_as_off_screen() {
        // The inset is a band, not a line: a marker that vanished the instant a target crossed the
        // window edge would flicker on and off while the player pans, which reads as a rendering bug.
        let at = marker_placement(Some(Vec2::new(EDGE_INSET - 1.0, 360.0)), SIZE);
        assert!(at.is_some(), "inside the inset band still warrants a marker");
    }

    #[test]
    fn an_unprojectable_target_is_still_reported() {
        // Behind the camera. Nearly unreachable with a fixed iso rig, but "not visible" is the honest
        // answer and silently dropping the marker would be the one failure mode the player cannot
        // distinguish from "there is nothing to find".
        let at = marker_placement(None, SIZE).expect("an unresolvable target is still lost");
        assert_eq!(at, Vec2::new(SIZE.x * 0.5, SIZE.y - EDGE_INSET));
    }

    #[test]
    fn a_degenerate_window_does_not_panic_or_invert() {
        // `clamp` panics if min > max. A window smaller than twice the inset is absurd but reachable
        // (a dragged-tiny window), and the repo's no-panic rule has no exception for absurd input.
        let tiny = Vec2::new(10.0, 8.0);
        let at = marker_placement(Some(Vec2::new(-100.0, -100.0)), tiny).expect("off-screen");
        assert!(at.x.is_finite() && at.y.is_finite());
        assert!(at.x >= 0.0 && at.y >= 0.0);
    }

    #[test]
    fn the_pulse_stays_in_the_medium_contrast_band() {
        // Lewandowska et al. 2022: medium contrast is sufficient in the periphery, and sustained high
        // intensity causes irritation over extended use. This marker IS extended use, so it must
        // never reach full opacity and never drop to invisible.
        for i in 0..200 {
            let a = pulse_alpha(i as f32 * 0.037);
            assert!(a >= 0.44 && a <= 0.86, "alpha {a} left the medium band");
        }
    }

    #[test]
    fn every_marker_kind_is_in_all() {
        // `ALL` drives the spawn loop; a kind missing from it is a marker that never exists.
        assert_eq!(MarkerKind::ALL.len(), 2);
        for (i, a) in MarkerKind::ALL.iter().enumerate() {
            for b in &MarkerKind::ALL[i + 1..] {
                assert_ne!(a, b, "{a:?} appears twice");
            }
        }
    }
}
