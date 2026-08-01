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

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::layout::SiteLayout;
use super::nav::SiteNav;
use super::aperture::{ApertureQuad, ApertureUniform, AsyncApertureMaterial};
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
        // `leave_for_the_site` reads the binding table non-optionally, and the plugin that registers a
        // reader is what guarantees the resource exists — the same contract `camera` states.
        crate::input::claim_bindings(app);
        app.add_plugins(MaterialPlugin::<AsyncApertureMaterial>::default())
            .insert_resource(nav)
            .insert_resource(SiteLayoutRes(layout))
            .add_systems(Startup, spawn_site_geometry)
            .add_systems(Update, super::aperture::drive_aperture_charge)
            .add_systems(OnEnter(AppState::Site), focus_camera_on_site)
            .add_systems(OnEnter(AppState::InGame), return_camera_to_squad)
            // The outbound half of a visit. Gated on the two facts independently, exactly as
            // `ui::containment_hud` is: the player must be looking at the expedition, and one must
            // be live. `enter_the_door` below is the inbound half.
            .add_systems(
                Update,
                leave_for_the_site
                    .run_if(in_state(AppState::InGame))
                    .run_if(in_state(crate::session::RunState::Active)),
            )
            // The inbound half of the toggle. Separate from the chain below because it must also
            // require a live run — `command_avatar`/`drive_avatars`/`enter_the_door` all work at the
            // Site between expeditions, and this one deliberately does not.
            .add_systems(
                Update,
                return_to_the_expedition
                    .run_if(in_state(AppState::Site))
                    .run_if(in_state(crate::session::RunState::Active)),
            )
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
fn place(
    commands: &mut Commands,
    assets: &AssetServer,
    kit: &crate::site::kit::SiteKit,
    piece: SitePiece,
    at: Vec3,
    yaw_deg: f32,
) {
    // Owned: `AssetPath` would otherwise borrow from the kit resource and escape into the spawn.
    let scene: Handle<WorldAsset> =
        assets.load(GltfAssetLabel::Scene(0).from_asset(kit.glb(piece).to_owned()));
    commands
        .spawn((
            SiteVisual,
            // `y_offset` lifts floor INLAYS clear of the plate they sit on: the line decals and the
            // threshold light are the same 0.06 m thickness as the Ozea floor, so at y = 0 their top
            // faces are exactly coplanar with it and the depth winner is undefined. See
            // `KitPiece::y_offset` for why this is geometric rather than a depth bias.
            Transform::from_translation(at + Vec3::Y * kit.y_offset(piece))
                .with_rotation(Quat::from_rotation_y(yaw_deg.to_radians()))
                // The kit is authored at its own scale per piece; `y_scale` lifts architecture to
                // WALL_HEIGHT and leaves dressing at the size the artist made it.
                .with_scale(Vec3::new(1.0, kit.y_scale(piece), 1.0)),
            Visibility::Inherited,
        ))
        .with_child((WorldAssetRoot(scene), Transform::default()));
}

fn spawn_site_geometry(
    mut commands: Commands,
    assets: Res<AssetServer>,
    kit: Res<crate::site::SiteKitRes>,
    layout: Res<SiteLayoutRes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut aperture_mats: ResMut<Assets<AsyncApertureMaterial>>,
) {
    let l = &layout.0;
    // Lights first, because nothing below is visible without them — see `light_the_site`.
    light_the_site(&mut commands, l);
    for r in &l.floor {
        for c in r.cells() {
            place(&mut commands, &assets, &kit, SitePiece::Floor, l.cell_center(c), 0.0);
        }
    }
    for w in &l.walls {
        let at = l.cell_center(IVec2::new(w.cell.0, w.cell.1));
        place(&mut commands, &assets, &kit, w.piece, at, w.yaw);
    }
    for p in &l.props {
        place(&mut commands, &assets, &kit, p.piece, l.point(p.pos), p.yaw);
    }

    // The ASYNC door: a wide frame, plus the trigger volume inside it.
    let door_at = l.point(l.door.pos);
    place(&mut commands, &assets, &kit, SitePiece::WallDoorwayWide, door_at, l.door.yaw);
    let (hx, hy, hz) = l.door.trigger_half_extents;
    commands.spawn((
        SiteVisual,
        AsyncDoor { half_extents: Vec3::new(hx, hy, hz) },
        Transform::from_translation(door_at),
    ));
    // The aperture itself: a quad standing in the frame's opening, recessed a couple of centimetres so
    // the frame geometry crops its edges rather than the two z-fighting. Sized to `DOORWAY_HEIGHT` so
    // it fills the opening the kit actually leaves.
    let opening_w = hx * 2.0;
    let opening_h = crate::dungeon::DOORWAY_HEIGHT;
    let quad = meshes.add(Rectangle::new(opening_w, opening_h));
    let mat = aperture_mats.add(AsyncApertureMaterial { settings: ApertureUniform::default() });
    commands.spawn((
        SiteVisual,
        ApertureQuad,
        Mesh3d(quad),
        MeshMaterial3d(mat),
        NotShadowCaster, // anomalous portal quad: casts no shadow (see world::setup_lighting)
        Transform::from_translation(door_at + Vec3::new(0.0, opening_h * 0.5, 0.02))
            .with_rotation(Quat::from_rotation_y(l.door.yaw.to_radians())),
    ));

    // Containment cells: the glazed front, and an empty marker the specimen body will fill.
    for c in &l.cells {
        let at = l.point(c.pos);
        place(&mut commands, &assets, &kit, SitePiece::WallWindow, at, c.yaw);
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

/// Ceiling height for the Site's own fixtures — just above `WALL_HEIGHT` so a light hangs over the
/// wall tops rather than inside them.
const SITE_FIXTURE_Y: f32 = 2.6;

/// Grid spacing (metres) between fixtures within an area. Matched to [`SITE_FIXTURE_RANGE`] so pools
/// overlap slightly instead of leaving dark bands between them.
const SITE_FIXTURE_SPACING: f32 = 7.0;

/// Per-fixture reach (metres). Same figure the dungeon's fixtures use — the rooms are a comparable
/// scale and a second, differently-tuned number would drift from it.
const SITE_FIXTURE_RANGE: f32 = 7.0;

/// Per-fixture luminous power (lumens), matching the dungeon's `fixture_intensity`.
const SITE_FIXTURE_LUMENS: f32 = 120_000.0;

/// **Light Site-67 from inside itself.**
///
/// Before this the hub had no light source at all. It was lit only by `world`'s single
/// `DirectionalLight`, which is aimed at the dungeon's origin — **1024 m away** — with a
/// `CascadeShadowConfig` sized for dungeon distances. So the Site received a grazing key it was never
/// pointed at, no shadows worth the name, and read as flat charcoal no matter what geometry stood in
/// it. Dressing the kit could not have fixed that; it is a lighting bug wearing an art costume.
///
/// **Derived from `layout.areas`, not hand-placed.** Every area gets covered by construction, so a
/// wing added to `site67.ron` later cannot ship unlit — the same argument that makes the perimeter
/// generated rather than typed. It also means the count scales with the floorplan instead of with
/// whoever last edited the list.
///
/// **Colour is the deliberate contrast.** The dungeon's fixtures are `(0.92, 1.0, 0.94)` — a faint
/// green cast, low-CRI halophosphate, chosen to feel sickly. The Site is the opposite claim: a clean,
/// faintly cool white. The player should be able to tell which world they are standing in with the HUD
/// switched off.
///
/// **No shadows, and that is a budget decision rather than an oversight.** `light::spawn_fixture_lights`
/// sets `shadow_maps_enabled: false` on every dungeon fixture for the same reason, and this adds ~29
/// lights that are always resident (the dungeon's spawn only as rooms are revealed). Clinical
/// fluorescent lighting is close to shadowless anyway, so the cheap answer is also the right look.
fn light_the_site(commands: &mut Commands, l: &SiteLayout) {
    let color = Color::srgb(0.96, 0.98, 1.0);
    for area in &l.areas {
        let r = &area.rect;
        // At least one fixture per area however small, then one per `SPACING` in each axis.
        let nx = ((r.w as f32) / SITE_FIXTURE_SPACING).ceil().max(1.0) as i32;
        let nz = ((r.h as f32) / SITE_FIXTURE_SPACING).ceil().max(1.0) as i32;
        for ix in 0..nx {
            for iz in 0..nz {
                // Centre each fixture in its share of the rect rather than on a corner, so an area
                // narrower than one spacing step still gets lit down its middle.
                let fx = r.x as f32 + (ix as f32 + 0.5) * (r.w as f32 / nx as f32);
                let fz = r.z as f32 + (iz as f32 + 0.5) * (r.h as f32 / nz as f32);
                let at = l.point((fx, fz)) + Vec3::Y * SITE_FIXTURE_Y;
                commands.spawn((
                    SiteVisual,
                    PointLight {
                        color,
                        intensity: SITE_FIXTURE_LUMENS,
                        range: SITE_FIXTURE_RANGE,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(at),
                ));
            }
        }
    }
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

/// Leave for Site-67 **without ending the expedition** (`docs/2026-08-01-two-live-layers.md` §2).
///
/// The whole flip is what this does *not* touch: `RunState` stays `Active`, so every system gated
/// `in_state(RunState::Active)` — the entire simulation — keeps ticking unattended, `run_scoped()`
/// despawns nothing, and `session::advance_to_next_world` never fires, so the world and its seed are
/// still there when you walk back. Ending a run is a separate, deliberate verb (`ui::pause`'s
/// `ABANDON EXPEDITION`); this one only changes which screen the player is on.
///
/// Deliberately **not** routed through the pause menu: `MenuState` is blocking, so a menu-gated visit
/// would hand out a free freeze on the way out and the squad would never actually be exposed.
fn leave_for_the_site(
    actions: crate::input::Actions,
    mut next_app: ResMut<NextState<AppState>>,
    mut onboarding: ResMut<crate::settings::OnboardingSettings>,
) {
    if actions.just_pressed(crate::input::Action::VisitSite) {
        info!("site: departing for SITE-67 — the expedition continues unattended");
        next_app.set(AppState::Site);
        // The player has demonstrably learned this verb, so its hint retires (`ui::hint`). Guarded on
        // the current value: `ResMut` marks changed on *deref*, and `settings::autosave_on_change`
        // writes the file whenever the resource changes — an unconditional assignment would rewrite
        // `user_settings.ron` on every single visit for the rest of the campaign.
        if !onboarding.learned_visit {
            onboarding.learned_visit = true;
        }
    }
}

/// The other half of the toggle: the same key carries the player back to the expedition.
///
/// Gated on `RunState::Active` as well as `AppState::Site`, and that is the whole rule — with no run
/// live there is nothing to return *to*, and the key must stay inert so the player standing in the hub
/// between expeditions cannot land on an empty `InGame` screen. Starting a fresh run from `Idle`
/// remains the ASYNC door's job ([`enter_the_door`]), which is a walk rather than a keystroke because
/// beginning an expedition should cost more than glancing at one.
///
/// This does not duplicate the door's `Active` arm so much as give it a shortcut: both set
/// `AppState::InGame` and touch nothing else, so there is exactly one transition with two triggers.
fn return_to_the_expedition(
    actions: crate::input::Actions,
    mut next_app: ResMut<NextState<AppState>>,
    mut onboarding: ResMut<crate::settings::OnboardingSettings>,
) {
    if actions.just_pressed(crate::input::Action::VisitSite) {
        info!("site: returning to the expedition");
        next_app.set(AppState::InGame);
        // Its own flag, and guarded, for the reasons given on `leave_for_the_site`.
        if !onboarding.learned_return {
            onboarding.learned_return = true;
        }
    }
}

/// Put the camera back on the squad when the expedition screen comes up.
///
/// The camera is deliberately not `run_scoped()` (`camera.rs`), so after a visit it is still parked at
/// the Site — 512+ world units away, per `site::layout`'s origin. A **snap**, not a glide: a glide
/// across that gap would be a long crawl over empty space, and this is a screen arrival, which is
/// exactly when `focus_camera_on_site` snaps in the other direction.
///
/// Harmless on the first entry of a run: `focus_camera_on_spawn` has already aimed at the dungeon
/// spawn on `OnEnter(RunState::Active)`, and the squad anchor is that same place before anyone moves.
fn return_camera_to_squad(
    anchor: Option<Res<crate::squad_ai::cohesion::SquadAnchor>>,
    mut rig: ResMut<crate::camera::CameraRig>,
    mut cams: Query<&mut Transform, With<Camera3d>>,
) {
    // No valid anchor means no living squad to look at (`squad` clears it on an empty roster), and
    // the terminal screens own the view at that point. Leave the camera where it is.
    let Some(anchor) = anchor.filter(|a| a.valid) else { return };
    crate::camera::snap_camera_to(anchor.pos, &mut rig, &mut cams);
}

/// Walking an avatar into the aperture is the ASYNC door, and it means one of two things depending on
/// whether an expedition is already live.
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
    let entered = avatars.iter().any(|a| {
        doors.iter().any(|(d, dt)| {
            let rel = (a.translation - dt.translation).abs();
            rel.x <= d.half_extents.x && rel.z <= d.half_extents.z
        })
    });
    if !entered {
        return;
    }
    // A `match`, not a guard. This was `if run_state != Idle { return }`, which was correct only while
    // standing at the Site implied no live expedition — after `leave_for_the_site` that guard would
    // strand the player at the Site with an expedition running and no way back to it.
    match *run_state.get() {
        // Nothing running: the door starts one. Note the `set` stays off the state we are already in —
        // a same-state transition rebuilds the world for nothing, the trap `ui::title` records.
        crate::session::RunState::Idle => {
            info!("site: an operative stepped through the ASYNC door — beginning an expedition");
            next_run.set(crate::session::RunState::Active);
            next_app.set(AppState::Warmup);
        }
        // A visit is ending: return to the expedition that has been running the whole time. `RunState`
        // is untouched, and the target is `InGame` and **not** `Warmup` — `Warmup` waits on `MoldWarm`
        // before handing over, which is right for a world being built and wrong for one already built.
        crate::session::RunState::Active => {
            info!("site: back through the ASYNC door — rejoining the expedition");
            next_app.set(AppState::InGame);
        }
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
    kit: Res<crate::site::SiteKitRes>,
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
                assets.load(
                    GltfAssetLabel::Scene(0)
                        .from_asset(kit.glb(SitePiece::SpecimenStandin).to_owned()),
                ),
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
