//! **Site-67 on screen** — geometry, operative avatars, the ASYNC door, and specimens in their cells.
//!
//! Windowed-only. Every system here is `Startup`/`Update`/`OnEnter`, never `FixedUpdate`, and nothing
//! it spawns carries `Health` — so none of it can contribute a row to `sim_harness::snapshot_hash` or an
//! actor to `liveness_violations`. That is a property of the **entity shapes**, not of the registration
//! site, so it survives someone moving this plugin.
//!
//! ## Spawned once at `Startup`, not per visit
//!
//! The Site is persistent by definition, so building it once and leaving it there is the literal
//! reading — and it avoids a despawn/respawn cycle that would re-instantiate ~150 GLB scenes on every
//! `RETURN TO SITE`. It sits 1 km from any dungeon (`layout.origin`), which is also how it stays out of
//! fog, the knee-wall cutaway, the light field, mould and almond water: all of those are dungeon-grid
//! indexed and simply never reach it. One mechanism — distance — instead of a `Visibility` toggle on
//! every Site entity at every state change.
//!
//! ## Avatars are not `Unit`s
//!
//! [`SiteAvatar`] is its own component. Real squad `Unit`s cannot stand here: `squad::unit_movement` and
//! `fog::update_los` both take `Res<Dungeon>`, which while `Idle` describes a despawned world. Promoting
//! avatars into persistent operatives is FVS-G-3's job; this keeps the door open for it by index-keying
//! them the same way `SquadMember` does.

use bevy::prelude::*;

use super::layout::SiteLayout;
use super::nav::SiteNav;
use super::pieces::SitePiece;
use crate::ui::state::AppState;

/// Collision half-extent for an avatar, matching the squad's own footprint so the Site's doorways feel
/// the same width as the dungeon's.
const AVATAR_HALF: f32 = 0.25;
/// Metres per second. A shade brisker than the expedition walk — nobody wants to trudge a hub.
const AVATAR_SPEED: f32 = 3.2;
/// How close counts as arrived, so an avatar does not jitter on its target.
const ARRIVE_EPS: f32 = 0.15;
/// The Valkyrie figurine's authored render scale (mirrors `squad`'s, so an operative is the same size
/// here as in the field).
const FIGURINE_SCALE: f32 = 1.13;

/// Marker on any entity belonging to the Site's presentation.
#[derive(Component)]
pub struct SiteVisual;

/// An operative standing in the hub. **Never `squad::Unit`** — see the module note.
///
/// Index-keyed like `squad::SquadMember` so FVS-G-3 can later map an avatar onto a persistent operative
/// without re-keying anything.
#[derive(Component, Debug, Clone, Copy)]
pub struct SiteAvatar(pub usize);

/// Where this avatar is walking, if anywhere.
#[derive(Component, Debug, Default)]
pub struct AvatarGoal(pub Option<Vec3>);

/// The one avatar the player drives.
#[derive(Component)]
pub struct PlayerAvatar;

/// The ASYNC door's trigger volume.
///
/// The same shape as `containment::Quarantine` — a `Transform` plus half-extents — because that is the
/// in-repo idiom for "a region that notices things", and reusing it means one mental model.
#[derive(Component, Debug, Clone, Copy)]
pub struct AsyncDoor {
    pub half_extents: Vec3,
}

/// A containment cell that can display one specimen, in authored display order.
#[derive(Component, Debug, Clone, Copy)]
pub struct ContainmentCell {
    pub index: u32,
    pub pos: Vec3,
}

/// The body currently shown inside a cell, if that cell is occupied.
#[derive(Component)]
pub struct CellOccupant;

pub struct SiteVisualsPlugin;

impl Plugin for SiteVisualsPlugin {
    fn build(&self, app: &mut App) {
        // Load and validate the layout at plugin build, so a malformed hub is a startup failure rather
        // than a half-built room — the same one-path stance as `config::load_game_config`.
        let layout = match SiteLayout::load() {
            Ok(l) => l,
            Err(e) => {
                error!("site: {e} — Site-67 will not be built");
                return;
            }
        };
        let nav = SiteNav::bake(&layout);
        app.insert_resource(nav)
            .insert_resource(SiteLayoutRes(layout))
            .add_systems(Startup, spawn_site_geometry)
            .add_systems(OnEnter(AppState::Site), focus_camera_on_site)
            .add_systems(
                Update,
                (command_avatar, drive_avatars, enter_the_door)
                    .chain()
                    .run_if(in_state(AppState::Site)),
            )
            // Cells fill outside the Site state too, so walking in never catches them mid-populate.
            .add_systems(Update, fill_containment_cells);
    }
}

/// The authored layout, kept for the systems that need world positions.
#[derive(Resource, Deref)]
pub struct SiteLayoutRes(pub SiteLayout);

/// One GLB piece, placed. The scene rides a **cosmetic child** — the same discipline every creature
/// spawn uses, because an async scene load attaching `Children`/`SceneInstance` to an entity other
/// systems query is the archetype churn `sim_harness` was hardened against.
fn place(commands: &mut Commands, assets: &AssetServer, piece: SitePiece, at: Vec3, yaw_deg: f32) {
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(piece.glb()));
    commands
        .spawn((
            SiteVisual,
            Transform::from_translation(at)
                .with_rotation(Quat::from_rotation_y(yaw_deg.to_radians()))
                // The kit is a 1 m module in every axis; `y_scale` lifts architecture to WALL_HEIGHT.
                .with_scale(Vec3::new(1.0, piece.y_scale(), 1.0)),
            Visibility::Inherited,
        ))
        .with_child((WorldAssetRoot(scene), Transform::default()));
}

fn spawn_site_geometry(mut commands: Commands, assets: Res<AssetServer>, layout: Res<SiteLayoutRes>) {
    let l = &layout.0;
    for r in &l.floor {
        for c in r.cells() {
            place(&mut commands, &assets, SitePiece::Floor, l.cell_center(c), 0.0);
        }
    }
    for w in &l.walls {
        let at = l.cell_center(IVec2::new(w.cell.0, w.cell.1));
        place(&mut commands, &assets, w.piece, at, w.yaw);
    }
    for p in &l.props {
        place(&mut commands, &assets, p.piece, l.point(p.pos), p.yaw);
    }

    // The ASYNC door: a wide frame, plus the trigger volume inside it.
    let door_at = l.point(l.door.pos);
    place(&mut commands, &assets, SitePiece::WallDoorwayWide, door_at, l.door.yaw);
    let (hx, hy, hz) = l.door.trigger_half_extents;
    commands.spawn((
        SiteVisual,
        AsyncDoor { half_extents: Vec3::new(hx, hy, hz) },
        Transform::from_translation(door_at),
    ));

    // Containment cells: the glazed front, and an empty marker the specimen body will fill.
    for c in &l.cells {
        let at = l.point(c.pos);
        place(&mut commands, &assets, SitePiece::WallWindow, at, c.yaw);
        commands.spawn((SiteVisual, ContainmentCell { index: c.index, pos: at }, Transform::from_translation(at)));
    }

    // Operative avatars. The first is the one the player drives.
    for (i, s) in l.spawns.iter().enumerate() {
        let at = l.point(*s);
        let mut e = commands.spawn((
            SiteVisual,
            SiteAvatar(i),
            AvatarGoal::default(),
            Transform::from_translation(at),
            Visibility::Inherited,
        ));
        e.with_child((
            WorldAssetRoot(
                assets.load(GltfAssetLabel::Scene(0).from_asset("characters/valkyrie.glb")),
            ),
            Transform::from_scale(Vec3::splat(FIGURINE_SCALE))
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
        ));
        if i == 0 {
            e.insert(PlayerAvatar);
        }
    }
    info!("site: built Site-67 ({} floor runs, {} cells)", l.floor.len(), l.cells.len());
}

fn focus_camera_on_site(
    layout: Res<SiteLayoutRes>,
    mut rig: ResMut<crate::camera::CameraRig>,
    mut cams: Query<&mut Transform, With<Camera3d>>,
) {
    // Aim at the spine's middle so all six areas are within a short pan.
    let l = &layout.0;
    let focus = l.cell_center(IVec2::new(16, 13));
    crate::camera::snap_camera_to(focus, &mut rig, &mut cams);
}

/// Left-click sets the player avatar's destination — the same verb the expedition uses for move orders,
/// so the hub needs no new control to learn.
fn command_avatar(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    nav: Res<SiteNav>,
    mut avatars: Query<&mut AvatarGoal, With<PlayerAvatar>>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, cam_tf) = *camera;
    let Some(cursor) = window.cursor_position() else { return };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else { return };
    let Some(d) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)) else { return };
    let point = ray.get_point(d);
    if !nav.is_walkable(nav.world_to_cell(point)) {
        return; // clicking a wall is not an order
    }
    for mut goal in &mut avatars {
        goal.0 = Some(point);
    }
}

fn drive_avatars(
    time: Res<Time>,
    nav: Res<SiteNav>,
    mut avatars: Query<(&mut Transform, &mut AvatarGoal), With<SiteAvatar>>,
) {
    let dt = time.delta_secs();
    for (mut tf, mut goal) in &mut avatars {
        let Some(target) = goal.0 else { continue };
        let to = (target - tf.translation).with_y(0.0);
        if to.length() <= ARRIVE_EPS {
            goal.0 = None;
            continue;
        }
        let step = to.normalize_or_zero() * AVATAR_SPEED * dt;
        let before = tf.translation;
        tf.translation = nav.resolve_move(before, step, AVATAR_HALF);
        // Face the way we actually moved, not the way we wanted to — sliding along a wall should turn
        // the avatar along the wall.
        let moved = (tf.translation - before).with_y(0.0);
        if moved.length_squared() > 1.0e-8 {
            tf.rotation = Quat::from_rotation_y(moved.x.atan2(moved.z));
        } else {
            // Wedged in a corner: drop the order rather than vibrate against the wall forever.
            goal.0 = None;
        }
    }
}

/// Walking an avatar into the aperture starts an expedition.
///
/// **No new state machinery** — FVS-A-5 already implements `Idle → Active` end to end, so the door is
/// that transition with a body. The `in_state(AppState::Site)` run condition is also the re-fire guard:
/// leaving the Site state stops this system, so there is no bool to keep in step.
fn enter_the_door(
    doors: Query<(&AsyncDoor, &Transform)>,
    avatars: Query<&Transform, With<SiteAvatar>>,
    run_state: Res<State<crate::session::RunState>>,
    mut next_run: ResMut<NextState<crate::session::RunState>>,
    mut next_app: ResMut<NextState<AppState>>,
) {
    // Only from `Idle`: a plain `set` to the state we are already in fires a same-state transition,
    // which rebuilds the world for nothing. That is the trap `ui::title` records.
    if *run_state.get() != crate::session::RunState::Idle {
        return;
    }
    let entered = avatars.iter().any(|a| {
        doors.iter().any(|(d, dt)| {
            let rel = (a.translation - dt.translation).abs();
            rel.x <= d.half_extents.x && rel.z <= d.half_extents.z
        })
    });
    if entered {
        info!("site: an operative stepped through the ASYNC door — beginning an expedition");
        next_run.set(crate::session::RunState::Active);
        next_app.set(AppState::Warmup);
    }
}

/// Show one body per held specimen, filling cells in **capture order**.
///
/// Capture order, not roster order: `SiteSpecimens` is a Bevy relationship target whose order is *attach*
/// order, so cell assignment would otherwise shuffle between sessions. `Specimen::captured_tick` exists
/// for exactly this.
fn fill_containment_cells(
    mut commands: Commands,
    assets: Res<AssetServer>,
    site: Option<Res<crate::site::SiteRoot>>,
    rosters: Query<&crate::site::SiteSpecimens>,
    specimens: Query<&crate::containment::Specimen>,
    cells: Query<(Entity, &ContainmentCell)>,
    occupied: Query<&ChildOf, With<CellOccupant>>,
) {
    let Some(site) = site else { return };
    // `Option`, always: Bevy REMOVES the relationship target when it empties, so a Site holding nothing
    // matches nothing — which reads as "no Site" if you query it bare. That is the first expedition.
    let Ok(roster) = rosters.get(site.0) else { return };

    let mut held: Vec<(u64, Entity)> = roster
        .iter()
        .filter_map(|e| specimens.get(e).ok().map(|s| (s.captured_tick, s.captured)))
        .collect();
    // SORT-OK: `(captured_tick, captured)` is total — an anomaly cannot be captured twice, because
    // `Contained` is inserted once and never removed.
    held.sort_unstable_by_key(|(tick, captured)| (*tick, *captured));

    let filled: std::collections::HashSet<Entity> = occupied.iter().map(|c| c.parent()).collect();
    let mut by_index: Vec<(Entity, &ContainmentCell)> = cells.iter().collect();
    // SORT-OK: authored display indices, validated dense and unique by `SiteLayout::validate`.
    by_index.sort_unstable_by_key(|(_, c)| c.index);

    for (slot, (cell_entity, cell)) in by_index.iter().enumerate() {
        if slot >= held.len() {
            break;
        }
        if filled.contains(cell_entity) {
            continue;
        }
        commands.entity(*cell_entity).with_child((
            CellOccupant,
            SiteVisual,
            Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)).with_scale(Vec3::splat(0.6)),
            Visibility::Inherited,
            WorldAssetRoot(
                assets.load(GltfAssetLabel::Scene(0).from_asset(SitePiece::SpecimenStandin.glb())),
            ),
        ));
        let _ = cell.pos;
    }
    if held.len() > by_index.len() {
        warn!(
            "site: {} specimens held but only {} cells authored — the extra records still exist, they \
             just have no body (raise the cell count in site67.ron)",
            held.len(),
            by_index.len()
        );
    }
}
