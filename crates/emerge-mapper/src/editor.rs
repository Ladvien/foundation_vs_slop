//! **The editing loop** — a palette, a ghost, a click, a file.
//!
//! # The ghost is the contract
//!
//! Everything an author does here goes through a preview that stands where the thing will actually
//! be. That is not polish; it is the rule `containment::cordon` states and the Site editor had to
//! learn twice: a preview drawn somewhere the piece will not end up *"is worse than no preview,
//! because it is a promise the game then breaks."* So the ghost is snapped, aimed, and lifted onto
//! its host exactly as the real placement will be.
//!
//! # Aiming happens before placing
//!
//! `[` and `]` turn the **brush**, never the last thing placed. The Site editor bound them to the
//! selection and it made rotation feel broken: placing selects, so the next `]` turned the piece you
//! had just put down while the ghost — the only thing on screen showing a facing — sat still.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button as UiButton, ScrollArea};
use emerge_core::descriptor::Mount;
use emerge_core::map::Placed;

use crate::project::Project;
use crate::view::{cursor_ground, MainCamera};

/// Translation snap, metres. Half a metre is the unit the kits are authored on.
const SNAP: f32 = 0.5;
/// Yaw snap, degrees.
const YAW_STEP: f32 = 15.0;

const PANEL_BG: Color = Color::srgb(0.058, 0.054, 0.047);
const ROW_BG: Color = Color::srgb(0.098, 0.092, 0.082);
const ROW_ARMED: Color = Color::srgb(0.30, 0.28, 0.24);
const TEXT: Color = Color::srgb(0.86, 0.84, 0.80);
const TEXT_DIM: Color = Color::srgb(0.58, 0.56, 0.53);
const ACCENT: Color = Color::srgb(0.90, 0.66, 0.24);

#[derive(Resource)]
pub struct EditorState {
    /// Index into the library — what a click would place.
    pub brush: usize,
    pub brush_yaw: f32,
    pub status: String,
    /// Monotonic counter behind generated placement ids, so two crates never share a name.
    next_id: u32,
}

impl Default for EditorState {
    fn default() -> Self {
        EditorState {
            brush: 0,
            brush_yaw: 0.0,
            status: String::new(),
            next_id: 0,
        }
    }
}

/// A palette row, carrying its library index so one observer can serve all of them.
#[derive(Component, Clone, Copy)]
struct PaletteRow(usize);

/// A spawned instance of a map placement, tagged with the id it came from.
#[derive(Component)]
pub struct Placement(pub String);

/// The see-through preview of the armed brush.
#[derive(Component)]
struct Ghost;

/// Which descriptor the live ghost is showing, so it is rebuilt only when the brush changes —
/// respawning a GLB every frame would thrash the asset server and never finish loading.
#[derive(Component)]
struct GhostOf(usize);

#[derive(Component)]
struct Ghosted;

#[derive(Component)]
struct StatusLine;

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
            .add_systems(Startup, (spawn_panel, spawn_existing).chain())
            .add_systems(
                Update,
                (
                    keys,
                    place_on_click,
                    drive_ghost,
                    fade_ghost,
                    style_rows,
                    refresh_status,
                ),
            )
            .add_observer(on_row_click);
    }
}

// ── chrome ───────────────────────────────────────────────────────────────────────────────────────

fn spawn_panel(mut commands: Commands, project: Res<Project>) {
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                top: Val::Px(12.0),
                width: Val::Px(300.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(12.0)),
                ..default()
            },
            // Opaque, not the game's translucent HUD panel. An editor panel is a work surface, and a
            // researcher in a white coat behind a translucent one is unreadable — measured.
            BackgroundColor(PANEL_BG),
            GlobalZIndex(100),
        ))
        .id();

    commands.entity(root).with_children(|p| {
        p.spawn((
            Text::new("EMERGE MAPPER"),
            TextColor(ACCENT),
            TextFont::from_font_size(15.0),
        ));
        p.spawn((
            Text::new("click place  |  [ ] aim  |  Q/E turn view\nWASD pan  |  wheel zoom  |  Ctrl+S save"),
            TextColor(TEXT_DIM),
            TextFont::from_font_size(11.0),
        ));
        p.spawn((
            Text::new(""),
            TextColor(TEXT),
            TextFont::from_font_size(12.0),
            StatusLine,
        ));

        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                max_height: Val::Px(520.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollArea::default(),
        ))
        .with_children(|list| {
            for (ix, d) in project.library.descriptors.iter().enumerate() {
                list.spawn((
                    UiButton,
                    Hovered::default(),
                    PaletteRow(ix),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(ROW_BG),
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new(d.id.clone()),
                        TextColor(TEXT),
                        TextFont::from_font_size(11.0),
                    ));
                });
            }
        });
    });
}

/// One observer for the whole palette. `Activate` carries the entity, so the index lives on the row
/// as a component and there is no macro-unrolled observer per row.
fn on_row_click(
    activate: On<Activate>,
    rows: Query<&PaletteRow>,
    project: Res<Project>,
    mut state: ResMut<EditorState>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    state.brush = row.0;
    if let Some(d) = project.library.descriptors.get(row.0) {
        state.status = format!("{} armed", d.id);
    }
}

fn style_rows(
    state: Res<EditorState>,
    mut rows: Query<(&PaletteRow, &Hovered, &mut BackgroundColor)>,
) {
    for (row, hovered, mut bg) in &mut rows {
        let want = if row.0 == state.brush {
            ROW_ARMED
        } else if hovered.0 {
            Color::srgb(0.16, 0.15, 0.14)
        } else {
            ROW_BG
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

fn refresh_status(
    project: Res<Project>,
    state: Res<EditorState>,
    mut lines: Query<&mut Text, With<StatusLine>>,
) {
    let brush = project
        .library
        .descriptors
        .get(state.brush)
        .map(|d| d.id.as_str())
        .unwrap_or("—");
    let want = format!(
        "{brush}  |  {} deg\n{} placed{}\n{}",
        state.brush_yaw,
        project.map.placements.len(),
        if project.dirty { "  |  UNSAVED" } else { "" },
        state.status
    );
    for mut t in &mut lines {
        if t.0 != want {
            t.0 = want.clone();
        }
    }
}

// ── placing ──────────────────────────────────────────────────────────────────────────────────────

fn snap(v: f32) -> f32 {
    (v / SNAP).round() * SNAP
}

/// Where a descriptor's origin goes for a given map position — the arithmetic the ghost and the real
/// placement both use, so they cannot disagree.
fn origin_of(d: &emerge_core::descriptor::Descriptor, at: (f32, f32)) -> Vec3 {
    let lift = match &d.mount {
        Some(Mount::OnWall { height }) => *height,
        Some(Mount::OnCeiling) => 2.4,
        _ => 0.0,
    };
    Vec3::new(at.0, lift + d.align.y_offset.unwrap_or(0.0), at.1)
}

fn spawn_piece(
    commands: &mut Commands,
    assets: &AssetServer,
    d: &emerge_core::descriptor::Descriptor,
    at: (f32, f32),
    yaw_deg: f32,
) -> Option<Entity> {
    let mesh = d.mesh.as_ref()?;
    let scene: Handle<WorldAsset> =
        assets.load(GltfAssetLabel::Scene(0).from_asset(mesh.clone()));
    let scale = d.align.scale.unwrap_or(1.0);
    // `front` is the mesh's own facing offset — the kit records it precisely so an authored yaw means
    // the same thing for every piece. Adding it here is what stops chairs being placed sideways.
    let yaw = yaw_deg + d.align.front.unwrap_or(0.0);
    Some(
        commands
            .spawn((
                Transform::from_translation(origin_of(d, at))
                    .with_rotation(Quat::from_rotation_y(yaw.to_radians()))
                    .with_scale(Vec3::splat(scale)),
                Visibility::Inherited,
            ))
            .with_child((WorldAssetRoot(scene), Transform::default()))
            .id(),
    )
}

/// Bring up whatever the map already holds.
fn spawn_existing(mut commands: Commands, assets: Res<AssetServer>, project: Res<Project>) {
    for p in &project.map.placements {
        let Some(d) = project.library.get(&p.descriptor) else {
            // Loud, not silent: a placement naming a descriptor the library does not have is a hole
            // in the map, and an author must be told which one rather than counting missing crates.
            warn!(
                "placement `{}` names descriptor `{}`, which this library does not have",
                p.id, p.descriptor
            );
            continue;
        };
        if let Some(e) = spawn_piece(&mut commands, &assets, d, p.at, p.yaw) {
            commands.entity(e).insert(Placement(p.id.clone()));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn place_on_click(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    assets: Res<AssetServer>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // A click on a control is not a click on the world. Without this, arming a piece from the palette
    // also dropped one wherever the panel happened to be over.
    if hovered_ui.iter().any(|h| h.0) {
        return;
    }
    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    let (cam, cam_tf) = *camera;
    let Some(hit) = cursor_ground(&window, cam, cam_tf) else {
        return;
    };

    let Some(d) = project.library.descriptors.get(state.brush).cloned() else {
        return;
    };
    let at = (snap(hit.x), snap(hit.z));
    state.next_id += 1;
    let id = format!("{}@{}", short_id(&d.id), state.next_id);

    let placed = Placed {
        id: id.clone(),
        descriptor: d.id.clone(),
        at,
        yaw: state.brush_yaw,
        ..Placed::default()
    };
    project.map.placements.push(placed);
    project.dirty = true;

    if let Some(e) = spawn_piece(&mut commands, &assets, &d, at, state.brush_yaw) {
        commands.entity(e).insert(Placement(id.clone()));
    }
    state.status = format!("placed {id} at ({}, {})", at.0, at.1);
}

/// The tail of a descriptor id, so a generated placement id reads as `crate@7` rather than
/// `kenney_prototype-kit/crate@7`.
fn short_id(descriptor_id: &str) -> &str {
    descriptor_id.rsplit('/').next().unwrap_or(descriptor_id)
}

fn keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut project: ResMut<Project>,
    mut state: ResMut<EditorState>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);

    if ctrl && keyboard.just_pressed(KeyCode::KeyS) {
        match project.save() {
            Ok(()) => {
                let path = project.map_path.display().to_string();
                state.status = format!("saved {path}");
                info!("saved {path}");
            }
            // The save refuses on an invalid map rather than writing one, and says which rule.
            Err(e) => {
                state.status = format!("NOT SAVED: {e}");
                error!("{e}");
            }
        }
        return;
    }

    let step = if keyboard.just_pressed(KeyCode::BracketRight) {
        YAW_STEP
    } else if keyboard.just_pressed(KeyCode::BracketLeft) {
        -YAW_STEP
    } else {
        0.0
    };
    if step != 0.0 {
        state.brush_yaw = (state.brush_yaw + step).rem_euclid(360.0);
    }
}

// ── the ghost ────────────────────────────────────────────────────────────────────────────────────

/// How much of its opacity a ghost keeps.
const GHOST_ALPHA: f32 = 0.45;

#[allow(clippy::too_many_arguments)]
fn drive_ghost(
    mut commands: Commands,
    assets: Res<AssetServer>,
    project: Res<Project>,
    state: Res<EditorState>,
    hovered_ui: Query<&Hovered>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    camera: Option<Single<(&Camera, &GlobalTransform), With<MainCamera>>>,
    ghosts: Query<(Entity, &GhostOf), With<Ghost>>,
    mut transforms: Query<&mut Transform, With<Ghost>>,
) {
    let clear = |commands: &mut Commands| {
        for (e, _) in &ghosts {
            commands.entity(e).despawn();
        }
    };

    let (Some(window), Some(camera)) = (window, camera) else {
        clear(&mut commands);
        return;
    };
    // No ghost while the cursor is over the panel: the piece is not going there, and a preview
    // hovering under the palette is a promise about a click that will never reach the world.
    if hovered_ui.iter().any(|h| h.0) {
        clear(&mut commands);
        return;
    }
    let (cam, cam_tf) = *camera;
    let Some(hit) = cursor_ground(&window, cam, cam_tf) else {
        clear(&mut commands);
        return;
    };
    let Some(d) = project.library.descriptors.get(state.brush) else {
        clear(&mut commands);
        return;
    };

    let at = (snap(hit.x), snap(hit.z));
    let yaw = state.brush_yaw + d.align.front.unwrap_or(0.0);

    let existing = ghosts.iter().find(|(_, g)| g.0 == state.brush).map(|(e, _)| e);
    for (e, g) in &ghosts {
        if g.0 != state.brush {
            commands.entity(e).despawn();
        }
    }

    match existing {
        Some(e) => {
            if let Ok(mut tf) = transforms.get_mut(e) {
                tf.translation = origin_of(d, at);
                tf.rotation = Quat::from_rotation_y(yaw.to_radians());
            }
        }
        None => {
            if let Some(e) = spawn_piece(&mut commands, &assets, d, at, state.brush_yaw) {
                commands.entity(e).insert((Ghost, GhostOf(state.brush)));
            }
        }
    }
}

/// Fade the ghost once its GLB has instantiated materials.
///
/// The materials come from the asset, so they are the *shared* handles every real instance uses —
/// writing alpha into them would turn every crate in the map see-through. Each descendant gets its
/// own clone, marked so the walk settles to a no-op.
fn fade_ghost(
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
                    faded.alpha_mode = AlphaMode::Blend;
                    let a = faded.base_color.alpha() * GHOST_ALPHA;
                    faded.base_color.set_alpha(a);
                    let handle = materials.add(faded);
                    commands.entity(e).insert((
                        MeshMaterial3d(handle),
                        Ghosted,
                        bevy::light::NotShadowCaster,
                    ));
                }
            }
            if let Ok(kids) = children.get(e) {
                queue.extend(kids.iter());
            }
        }
    }
}
