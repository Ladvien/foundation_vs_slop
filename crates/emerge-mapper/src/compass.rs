//! **Which way is which** — the navigation gizmo, bottom-left of the 3D view.
//!
//! Asked for at the keyboard, 2026-08-15: *"a widget that shows x, y, and z coordinates, kind of
//! like the compass... the circle and the three axes arrows."* Blender's navigation gizmo and Maya's
//! ViewCube are the same idea, and the reason this editor wants one is sharper than theirs: since
//! the arrows went camera-relative (`build::step_in_view` — *"up is whatever up looks like from
//! here"*), **the keys are described relative to a frame nothing on screen names.** `Q`/`E` turn
//! that frame in quarter detents and nothing moved with it.
//!
//! # It draws the camera's own answer, never its own
//!
//! Every arm comes from [`crate::view::axis_on_screen`], which reads the basis off [`view::Rig`] —
//! the same rig `pan_direction` reads, and the tests hold the two together on the horizontal. A
//! compass that derived its own angles would be a second answer to "which way is +X", and this
//! editor has paid for a second answer to a spatial question more than once.
//!
//! # Why arms and not a cube
//!
//! A cube needs faces, and faces need either a second camera rendering to a texture (the trap
//! `TransformGizmoPlugin` fell into — a second `Camera3d` breaks every `Single<.., With<Camera3d>>`
//! in the app, `CLAUDE.md` records the measurement) or a mesh in the world, which would scale with
//! zoom and sit behind the map. Three rotated UI nodes need neither: `UiTransform::rotation` is a
//! `Rot2` and the projection is four dot products.

use bevy::prelude::*;
use bevy::ui::{UiTransform, Val2};

use crate::chrome::{MARGIN, PANEL_BG};
use crate::view::Rig;

/// How wide the gizmo's box is, and therefore how long an arm can be.
const SIZE: f32 = 84.0;

/// How far an unforeshortened arm reaches from the centre. Short of `SIZE / 2` so a label at the
/// end of an arm still has room inside the box rather than clipping at its edge.
const REACH: f32 = 26.0;

/// The dot at the end of each arm, and the arm's own thickness.
const DOT: f32 = 14.0;
const ARM: f32 = 2.0;

/// **The conventional axis colours** — X red, Y green, Z blue, as every DCC tool from Blender to
/// Maya to Unity draws them. Deliberately NOT this editor's own palette: an author arrives already
/// knowing what a red arm means, and spending that knowledge is free.
// The doc above is the whole argument, and naming these in `chrome` would invite a future sweep to
// harmonise them with the palette — the one thing they must never do.
const X_AXIS: Color = Color::srgb(0.91, 0.30, 0.33); // CHROME-OK: DCC convention, not our palette
const Y_AXIS: Color = Color::srgb(0.45, 0.78, 0.35); // CHROME-OK: DCC convention, not our palette
const Z_AXIS: Color = Color::srgb(0.31, 0.55, 0.93); // CHROME-OK: DCC convention, not our palette

/// The three axes, in the order the arms are spawned — read by the update system through
/// [`CompassArm::0`], so the two cannot disagree about which arm is which.
const AXES: [(Vec3, Color, &str); 3] = [
    (Vec3::X, X_AXIS, "X"),
    (Vec3::Y, Y_AXIS, "Y"),
    (Vec3::Z, Z_AXIS, "Z"),
];

/// The gizmo's root, so the whole thing shows and hides as one.
#[derive(Component)]
struct Compass;

/// One axis's arm — the line from the centre outward. Carries its index into [`AXES`].
#[derive(Component)]
struct CompassArm(usize);

/// One axis's dot and label, at the end of the arm.
#[derive(Component)]
struct CompassDot(usize);

pub struct CompassPlugin;

impl Plugin for CompassPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(crate::screen::Screen::Editor), spawn)
            // `Update`, not `FixedUpdate`: this is chrome, and it follows a camera that eases.
            .add_systems(Update,
                ((follow_the_camera, place_by_tab))
                    .run_if(in_state(crate::screen::Screen::Editor)),
            );
    }
}

fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(MARGIN),
                bottom: Val::Px(MARGIN),
                width: Val::Px(SIZE),
                height: Val::Px(SIZE),
                // The circle the arms turn inside — the "circle" of the request, and what makes a
                // rotating cluster read as one object rather than three drifting dots. A `Node`
                // FIELD in 0.19, not a component of its own.
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(PANEL_BG.with_alpha(0.72)),
            GlobalZIndex(100),
            // A readout that eats clicks is a readout that steals the corner of the map underneath
            // it — the rule `spawn_cost_readout` states, and the same corner problem.
            Pickable::IGNORE,
            Compass,
        ))
        .with_children(|p| {
            // **Arms first, dots second**, so a dot draws over the arm that reaches it rather than
            // under it. Bevy UI paints siblings in spawn order.
            for (i, (_, colour, _)) in AXES.iter().enumerate() {
                p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(REACH),
                        height: Val::Px(ARM),
                        ..default()
                    },
                    BackgroundColor(*colour),
                    Pickable::IGNORE,
                    CompassArm(i),
                ));
            }
            for (i, (_, colour, name)) in AXES.iter().enumerate() {
                p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(DOT),
                        height: Val::Px(DOT),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(*colour),
                    // **Above every arm, said rather than implied.** Sibling spawn order already
                    // puts the dots second, and it was not enough: at some yaws an arm still drew
                    // across its own label. Reported from the keyboard, 2026-08-15 — *"one of the
                    // lines... actually sits over the letter."* The arm is shortened to stop at the
                    // dot's edge as well, so this is the belt to that braces: the letter is legible
                    // even if a future arm reaches further than it should.
                    ZIndex(1),
                    Pickable::IGNORE,
                    CompassDot(i),
                ))
                .with_children(|dot| {
                    dot.spawn((
                        Text::new(*name),
                        // This letter sits ON the axis dot, so it is read against X_AXIS/Y_AXIS/
                        // Z_AXIS and never against a panel; an ink picked to contrast with
                        // `PANEL_BG` would be the wrong contrast here.
                        // CHROME-OK: read against the axis dot, not against a panel.
                        TextColor(Color::srgb(0.10, 0.10, 0.10)),
                        TextFont::from_font_size(9.0),
                        Pickable::IGNORE,
                    ));
                });
            }
        });
}

/// **Place every arm and dot from the camera's basis.**
///
/// Runs each frame because the rig EASES between detents — `Q` does not snap, it swings, and a
/// gizmo that only updated at the detents would sit still through the part an author is watching.
fn follow_the_camera(
    rig: Res<Rig>,
    mut arms: Query<(&CompassArm, &mut Node, &mut UiTransform), Without<CompassDot>>,
    mut dots: Query<(&CompassDot, &mut Node, &mut BackgroundColor), Without<CompassArm>>,
) {
    let centre = SIZE * 0.5;
    for (arm, mut node, mut tf) in &mut arms {
        let Some((axis, ..)) = AXES.get(arm.0) else {
            continue;
        };
        let (on_screen, _) = crate::view::axis_on_screen(*axis, &rig);
        let reach = on_screen * REACH;
        // **The arm stops at the dot's edge, not at its centre.** Running it the whole way drew a
        // line straight through the letter — the two overlapped by half a dot at every yaw, and at
        // some angles the line won. Ending it short means the glyph sits on clean colour.
        let length = (reach.length() - DOT * 0.5).max(1.0);
        let dir = on_screen.normalize_or_zero();
        // A node rotates about its own centre, so the arm is placed with its MIDDLE half way along
        // that shortened length and then turned — one end at the gizmo's centre, the other at the
        // dot's near edge.
        node.width = Val::Px(length);
        node.left = Val::Px(centre + dir.x * length * 0.5 - length * 0.5);
        node.top = Val::Px(centre + dir.y * length * 0.5 - ARM * 0.5);
        // `Rot2` turns clockwise, and UI y grows down — the two flips cancel, so a plain `atan2`
        // of the UI-space direction is the angle, with no sign correction.
        tf.rotation = Rot2::radians(reach.y.atan2(reach.x));
        tf.translation = Val2::ZERO;
    }
    for (dot, mut node, mut colour) in &mut dots {
        let Some((axis, base, _)) = AXES.get(dot.0) else {
            continue;
        };
        let (on_screen, toward) = crate::view::axis_on_screen(*axis, &rig);
        let reach = on_screen * REACH;
        node.left = Val::Px(centre + reach.x - DOT * 0.5);
        node.top = Val::Px(centre + reach.y - DOT * 0.5);
        // **The arm going away from you is dimmer.** Without this all three read as equally near,
        // which is the one thing an isometric view cannot tell you by length: at this elevation the
        // three axes project to exactly equal lengths, by definition of *isometric*.
        let alpha = if toward >= 0.0 { 1.0 } else { 0.35 };
        let want = base.with_alpha(alpha);
        if colour.0 != want {
            colour.0 = want;
        }
    }
}

/// **One gizmo, on every tab that has a camera to be lost in — moved clear of that tab's panel.**
///
/// It is not one gizmo per tab, and the difference matters: there is a single `MainCamera` and a
/// single [`Rig`], `Q`/`E` turn it from anywhere, so a second compass would be a second answer to a
/// question that has one. What changes per tab is only where it can be *seen*.
///
/// Tiles and Meshes share a full-height left panel ([`crate::chrome::TILES_CONTROLS_W`] wide), so
/// the corner the Map leaves free is covered there — the gizmo steps right, to the inside edge of
/// that panel, which is still the bottom-left of the *viewport*. The Map's own left panel stops
/// short of the bottom, so it keeps the true corner.
///
/// Compose and Anim are deliberately absent for now: Anim drives its own camera presets and has its
/// own reading of "which way is the figure facing", and putting a second orientation cue beside that
/// is a question rather than an answer. Both are one arm of this match away if they want it.
fn place_by_tab(mode: Res<crate::tiles::Mode>, mut compass: Query<&mut Node, With<Compass>>) {
    if !mode.is_changed() {
        return;
    }
    use crate::tiles::Mode;
    // `None` is "this tab does not show it"; `Some(left)` is where it sits when it does.
    let want = match *mode {
        Mode::Map => Some(MARGIN),
        // Clear of the shared controls panel, with the same margin on the far side of it.
        Mode::Tiles | Mode::Meshes => Some(MARGIN + crate::chrome::TILES_CONTROLS_W + MARGIN),
        Mode::Compose | Mode::Anim => None,
    };
    for mut node in &mut compass {
        let display = if want.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Some(left) = want {
            let left = Val::Px(left);
            if node.left != left {
                node.left = left;
            }
        }
    }
}
