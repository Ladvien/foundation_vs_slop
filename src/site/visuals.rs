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

use super::aperture::{ApertureQuad, ApertureUniform, AsyncApertureMaterial};
use super::layout::SiteLayout;
use super::nav::SiteNav;
use super::pieces::SitePiece;
use crate::anim;
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
/// Time constant for smoothing an operative's measured speed before it drives the blend. Matches the
/// squad's `LOCO_SMOOTH_TAU` in spirit: long enough that a single stuttery frame cannot flip the pose.
const AVATAR_LOCO_TAU: f32 = 0.12;

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
    /// The cell's authored yaw, kept so the occupant can be placed INSIDE the booth rather than in
    /// the plane of its glass — see `cell_interior_dir`.
    pub yaw: f32,
}

/// The body currently shown inside a cell, if that cell is occupied.
#[derive(Component)]
pub struct CellOccupant;

/// The examination slab in the research wing. Spawned beside the `Slab` prop, at the height of its
/// bed platform, so a study subject can simply be parented to it.
#[derive(Component)]
pub struct StudySlab;

/// The body currently lying on the slab, if anything is being studied.
#[derive(Component)]
pub struct SlabOccupant;

/// The GLB child of a [`SiteAvatar`]. Carries the cosmetic animation state, never the avatar itself —
/// the same split `squad::FigurineModel` makes, and for the same reason (issue #18).
#[derive(Component)]
struct AvatarModel;

/// Smoothed locomotion for one operative's model, so the blend does not chatter frame to frame.
///
/// `last` is the parent's position at the previous frame: Site avatars are moved by writing
/// `Transform` directly (`drive_avatars`), so unlike a squad `Unit` there is no `Velocity` to read.
#[derive(Component, Default)]
struct AvatarLoco {
    speed: f32,
    last: Option<Vec3>,
}

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
            // AFTER the graph exists: `spawn_site_geometry` pins `ValkyrieAnim`'s graph and slots on
            // each operative's model child as an `anim::BlendSource`, and Bevy is otherwise free to
            // order two `Startup` systems either way round. The squad states the same constraint for
            // `spawn_unit`; the Site is the second spawner and needs it just as much.
            .add_systems(
                Startup,
                spawn_site_geometry.after(crate::squad::build_valkyrie_anim),
            )
            // Cosmetic, so `Update` — never `FixedUpdate` (`docs/animation.md`). Deliberately NOT
            // gated on `AppState::Site`: `apply_pose_blenders` snaps weights on its first pass, so a
            // blender that had never been driven would prime to all-zero and show one frame of bind
            // pose the moment the player walks in. Five avatars is not a cost worth that.
            .add_systems(Update, drive_avatar_animation)
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
            .add_systems(Update, (fill_containment_cells, lay_out_the_study_subject));
    }
}

/// The authored layout, kept for the systems that need world positions.
#[derive(Resource, Deref)]
pub struct SiteLayoutRes(pub SiteLayout);

/// The cells where two wall runs meet — derived from the layout, never authored.
///
/// A junction is a cell carrying **both** a yaw-0 and a yaw-90 [`SitePiece::Wall`]. That is how
/// `site67.ron` draws its 12 corners: two 0.10 m slabs crossed on one cell, which leaves the outer
/// corner as two exposed slab ends. The kit has always carried a `wall_corner` cap for exactly this,
/// it is validated, it is in `SitePiece::ALL` — and until now nothing ever placed one. Shipped,
/// dressed and unreachable.
///
/// **Derived rather than authored** so a future edit to the layout cannot forget a corner, and so the
/// piece is reachable by construction instead of by remembering. Yaw is compared on the half-turn
/// (`rem_euclid(180)`): a wall at 180° lies along the same axis as one at 0°, so treating them as
/// different orientations would invent junctions that are really just a slab facing the other way.
fn corner_cells(l: &SiteLayout) -> std::collections::HashSet<(i32, i32)> {
    let mut axes: std::collections::HashMap<(i32, i32), u32> = std::collections::HashMap::new();
    for w in &l.walls {
        if w.piece != SitePiece::Wall {
            continue; // a Column standing in the spine is not a junction
        }
        let half = w.yaw.rem_euclid(180.0);
        // Two axis bits, so "has both" is one mask test and a float yaw never reaches a hash key.
        let bit = if !(45.0..135.0).contains(&half) { 1 } else { 2 };
        *axes.entry(w.cell).or_insert(0) |= bit;
    }
    axes.into_iter()
        .filter(|(_, m)| *m == 3)
        .map(|(c, _)| c)
        .collect()
}

/// Which way a junction's cap faces, in degrees about +Y.
///
/// Derived from which neighbours continue a wall: the corner turns *away* from the two directions the
/// runs leave in. Yaw 0 is the corner whose arms point +X and +Z.
///
/// **The shipped Ozea cap is a 0.22 × 0.22 square post, so this yaw is currently invisible.** It is
/// computed anyway because the greybox kit's corner is an L (`kenney .../wall-corner.glb`), and an
/// L pointed the wrong way at three of four junctions is a silent, per-corner wrongness nobody would
/// trace back here. `SITE_KIT_PATH` is one line; this keeps a kit swap from needing a second one.
fn corner_yaw(l: &SiteLayout, cell: (i32, i32)) -> f32 {
    let has_wall = |c: (i32, i32)| {
        l.walls
            .iter()
            .any(|w| w.piece == SitePiece::Wall && w.cell == c)
    };
    let (x, z) = cell;
    let (px, nx) = (has_wall((x + 1, z)), has_wall((x - 1, z)));
    let (pz, nz) = (has_wall((x, z + 1)), has_wall((x, z - 1)));
    match (px, pz, nx, nz) {
        (true, true, _, _) => 0.0,
        (_, true, true, _) => 90.0,
        (_, _, true, true) => 180.0,
        (true, _, _, true) => 270.0,
        // Runs that do not leave in two perpendicular directions are a crossing, not a corner (the
        // spine's T-junctions). A square post is right there and orientation is moot.
        _ => 0.0,
    }
}

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

/// Bevy's `Rectangle` is authored in the XY plane with a **+Z normal**, so its width axis is world X.
/// Every doorframe in every kit here is thin along X and spans Z — Ozea's `SM_DoorFrame_Double` and
/// Kenney's `wall-doorway` alike — so a frame's opening plane faces ±X. The two native axes differ by
/// a quarter turn, and handing both the same unmodified `door.yaw` is what stood the quad *across* the
/// frame rather than in it, for ANY value of that yaw. `-90` puts the lit face toward the hall.
const APERTURE_QUAD_YAW_OFFSET: f32 = -90.0;

/// Metres the quad is pushed back INTO the frame, so the jambs crop its edges instead of the two
/// z-fighting. It rides the quad's own normal: as a bare world-space `+Z` nudge — which is what it
/// was — it slid sideways along the frame at every yaw but zero.
const APERTURE_RECESS: f32 = 0.02;

/// The ASYNC aperture: a quad standing in the doorframe's clear opening.
///
/// Sized from the kit's measured `opening`, which is an **art** fact about the frame. It was sized
/// from `DoorPlacement::trigger_half_extents` until 2026-08-01 — a gameplay volume, generous on
/// purpose so it catches a walking avatar — which made a 3.2 m quad for a 1.6 m hole. The material is
/// `AlphaMode::Opaque` deliberately (the aperture must occlude), so that overhang did not fade out at
/// the edges: it punched an opaque hole through the wall either side of the door.
///
/// `assets/shaders/async_aperture.wgsl` remaps `mesh.uv` to `[-1, 1]` and marches on `uv.x`, with no
/// aspect uniform to compensate — so the corridor illusion is stretched by whatever the quad's aspect
/// happens to be. At 1.600 × 1.642 that is very nearly square, which is what it was written assuming;
/// at the old 3.2 × 2.0 it was stretched 1.6:1. Tuning the shader itself is FVS-G-5 and still open.
fn spawn_aperture_quad(
    commands: &mut Commands,
    kit: &crate::site::kit::SiteKit,
    meshes: &mut Assets<Mesh>,
    aperture_mats: &mut Assets<AsyncApertureMaterial>,
    frame_at: Vec3,
    yaw_deg: f32,
) {
    // Width is authored as-is because `place` scales Y only; height rides the frame's own y_scale, so
    // the quad grows exactly as much as the opening it fills does.
    let (ow, oh) = kit.wall_doorway_wide.opening;
    let oh = oh * kit.y_scale(SitePiece::WallDoorwayWide);
    let quad = meshes.add(Rectangle::new(ow, oh));
    let mat = aperture_mats.add(AsyncApertureMaterial {
        settings: ApertureUniform::default(),
    });
    let rot = Quat::from_rotation_y((yaw_deg + APERTURE_QUAD_YAW_OFFSET).to_radians());
    let normal = rot * Vec3::Z;
    commands.spawn((
        SiteVisual,
        ApertureQuad,
        Mesh3d(quad),
        MeshMaterial3d(mat),
        NotShadowCaster, // anomalous portal quad: casts no shadow (see world::setup_lighting)
        // The opening starts at the floor, so the quad's centre is half its height up.
        Transform::from_translation(frame_at + Vec3::Y * oh * 0.5 - normal * APERTURE_RECESS)
            .with_rotation(rot),
    ));
}

/// Height of the examination slab's bed platform, in metres — where a study subject lies.
///
/// Measured off `slab.glb` (`SM_MedPod_Treatment_Bed`): the widest horizontal band of the mesh is at
/// y ≈ 0.55–0.60, which is the mattress. Not guessed, and not the mesh's 1.72 m overall height, which
/// is the raised canopy at the head end.
const SLAB_SURFACE_Y: f32 = 0.60;

/// How deep a containment booth runs behind its glazed front, in metres. Two cells, matching the 2 m
/// span of `wall_window` itself, so a cell is square in plan.
const CELL_DEPTH: f32 = 2.0;

/// Which way a containment cell's interior lies, given its authored yaw.
///
/// `wall_window` is thin along local X and 2 m long along local Z (same convention as every wall in
/// the kit), so the glass faces `rot(yaw) · X` and the booth runs the other way.
fn cell_interior_dir(yaw_deg: f32) -> Vec3 {
    -(Quat::from_rotation_y(yaw_deg.to_radians()) * Vec3::X)
}

/// Build the booth behind a containment cell's glass: two side walls and a back.
///
/// **Derived from the cell's own placement, never authored** — the same discipline `corner_cells`
/// uses, so adding a seventh cell to `site67.ron` gets an enclosure for free and no cell can be left
/// as a bare pane by forgetting to type one.
///
/// Until 2026-08-01 a cell WAS a bare pane: `site67.ron`'s `cells:` authors one `WallWindow` and
/// nothing around it, so a containment wing holding nothing read as six sheets of glass standing on
/// an open deck. That undercuts the whole point of FVS-D-4 — the player is supposed to walk past a
/// rack of the things they brought home, and a rack has to look like one when it is empty.
///
/// Sides run along the interior direction and are therefore a quarter-turn from the glass; the back
/// is parallel to it. Everything is `Wall`, which is 1 m long, so a 2 m run is two pieces.
fn enclose_containment_cell(
    commands: &mut Commands,
    assets: &AssetServer,
    kit: &crate::site::kit::SiteKit,
    at: Vec3,
    yaw_deg: f32,
) {
    let rot = Quat::from_rotation_y(yaw_deg.to_radians());
    let inward = cell_interior_dir(yaw_deg);
    let span = rot * Vec3::Z;
    // Sides: at both ends of the glass, stepping one metre at a time into the booth.
    for side in [-1.0f32, 1.0] {
        for step in [0.5f32, 1.5] {
            let p = at + span * side * (CELL_DEPTH * 0.5) + inward * step;
            place(commands, assets, kit, SitePiece::Wall, p, yaw_deg + 90.0);
        }
    }
    // Back: parallel to the glass, at the far end of the booth.
    for side in [-0.5f32, 0.5] {
        let p = at + inward * CELL_DEPTH + span * side;
        place(commands, assets, kit, SitePiece::Wall, p, yaw_deg);
    }
}

fn spawn_site_geometry(
    mut commands: Commands,
    assets: Res<AssetServer>,
    kit: Res<crate::site::SiteKitRes>,
    layout: Res<SiteLayoutRes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut aperture_mats: ResMut<Assets<AsyncApertureMaterial>>,
    valk: Res<crate::squad::ValkyrieAnim>,
) {
    let l = &layout.0;
    // Lights first, because nothing below is visible without them — see `light_the_site`.
    light_the_site(&mut commands, l);
    for r in &l.floor {
        for c in r.cells() {
            place(
                &mut commands,
                &assets,
                &kit,
                SitePiece::Floor,
                l.cell_center(c),
                0.0,
            );
        }
    }
    for w in &l.walls {
        let at = l.cell_center(IVec2::new(w.cell.0, w.cell.1));
        place(&mut commands, &assets, &kit, w.piece, at, w.yaw);
    }
    // Cap every junction. ADDITIVE — the two crossed slabs stay and the cap covers the seam where
    // they meet; it does not replace them. Substituting it was the first attempt and it was wrong:
    // `SM_Wall_CornerCap` is a 0.22 m post, so swapping it in deleted a full metre of wall from each
    // run and left a pole standing in the gap — which is precisely what the player had reported.
    for cell in corner_cells(l) {
        let at = l.cell_center(IVec2::new(cell.0, cell.1));
        place(
            &mut commands,
            &assets,
            &kit,
            SitePiece::WallCorner,
            at,
            corner_yaw(l, cell),
        );
    }
    for p in &l.props {
        let at = l.point(p.pos);
        place(&mut commands, &assets, &kit, p.piece, at, p.yaw);
        // The slab is the one prop with a gameplay meaning attached, so it also gets a marker at the
        // height of its bed platform for `lay_out_the_study_subject` to parent a body to.
        if p.piece == SitePiece::Slab {
            commands.spawn((
                SiteVisual,
                StudySlab,
                Transform::from_translation(at + Vec3::Y * SLAB_SURFACE_Y)
                    .with_rotation(Quat::from_rotation_y(p.yaw.to_radians())),
                Visibility::Inherited,
            ));
        }
    }

    // The ASYNC door: a wide frame standing IN the perimeter gap, plus the trigger volume on the floor
    // in front of it. Two positions, deliberately — `frame_pos` is not floor and `pos` must be, so one
    // field could never have served both. It served `pos`, and the frame stood a metre out in the hall.
    let frame_at = l.point(l.door.frame_pos);
    place(
        &mut commands,
        &assets,
        &kit,
        SitePiece::WallDoorwayWide,
        frame_at,
        l.door.yaw,
    );
    let (hx, hy, hz) = l.door.trigger_half_extents;
    commands.spawn((
        SiteVisual,
        AsyncDoor {
            half_extents: Vec3::new(hx, hy, hz),
        },
        Transform::from_translation(l.point(l.door.pos)),
    ));
    // The header course. The frame reaches `DOORWAY_HEIGHT` and the walls beside it `WALL_HEIGHT`, so
    // without this the perimeter has a 0.40 m slot straight through it above the door — you see the
    // void over the lintel. `DOORWAY_HEIGHT`'s doc has always said "the wall runs continuous above
    // it"; the dungeon honoured that and the Site never had. The cells come from the layout, so the
    // course cannot disagree with the gap `validate` checks the frame against.
    for cell in l.doorway_gap_cells() {
        let at = l.cell_center(cell) + Vec3::Y * crate::dungeon::DOORWAY_HEIGHT;
        place(
            &mut commands,
            &assets,
            &kit,
            SitePiece::WallHeader,
            at,
            l.door.yaw,
        );
    }
    spawn_aperture_quad(
        &mut commands,
        &kit,
        &mut meshes,
        &mut aperture_mats,
        frame_at,
        l.door.yaw,
    );

    // Containment cells: the glazed front, the booth behind it, and an empty marker the specimen body
    // will fill.
    for c in &l.cells {
        let at = l.point(c.pos);
        place(
            &mut commands,
            &assets,
            &kit,
            SitePiece::WallWindow,
            at,
            c.yaw,
        );
        enclose_containment_cell(&mut commands, &assets, &kit, at, c.yaw);
        commands.spawn((
            SiteVisual,
            ContainmentCell {
                index: c.index,
                pos: at,
                yaw: c.yaw,
            },
            Transform::from_translation(at),
        ));
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
            AvatarModel,
            WorldAssetRoot(
                assets.load(GltfAssetLabel::Scene(0).from_asset("characters/valkyrie.glb")),
            ),
            Transform::from_scale(Vec3::splat(FIGURINE_SCALE))
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
            // Without this the operatives stand in the GLB's BIND POSE — arms straight out, rifle
            // held level at chest height a metre to the side, which reads as a lance run through
            // each of them. The mesh was never wrong: `rifle` measures 0.902 m composed through its
            // whole node chain. Nothing was animating them, so nothing ever left the rest pose.
            //
            // Same seam the squad uses (`squad::spawn_unit`): the cosmetic state rides the MODEL
            // child, never the entity other systems query, and `anim::attach_pose_blenders` wires
            // the streamed-in `AnimationPlayer` to the nearest `BlendSource` ancestor — this entity.
            anim::BlendSource {
                graph: valk.graph.clone(),
                slots: valk.slots.clone(),
            },
            AvatarLoco::default(),
        ));
        if i == 0 {
            e.insert(PlayerAvatar);
        }
    }
    // The corner count is how the derived rule is verified at runtime — `site67.ron` authors no
    // corners at all, so a 0 here means the derivation stopped matching the layout.
    info!(
        "site: built Site-67 ({} floor runs, {} cells, {} wall corners)",
        l.floor.len(),
        l.cells.len(),
        corner_cells(l).len()
    );
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
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else {
        return;
    };
    let Some(d) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)) else {
        return;
    };
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

/// Put the specimen under study on the slab, and take it off again when nothing is being studied.
///
/// **Makes an existing system visible instead of inventing one.** `research::lab::StudySubject` is
/// documented as "the specimen currently on the slab", and the research HUD says *NO SPECIMEN ON THE
/// SLAB — CONTAIN ONE FIRST* — but until 2026-08-01 there was no slab anywhere in the world and the
/// research wing was bare floor. The subject is chosen by `research::lab::keep_a_study_subject`; this
/// only shows what it already decided, so it adds no gameplay rule of its own.
///
/// Cosmetic, `Update`, and it spawns nothing carrying `Health` — the module's invariant holds.
fn lay_out_the_study_subject(
    mut commands: Commands,
    assets: Res<AssetServer>,
    kit: Res<crate::site::SiteKitRes>,
    subject: Res<crate::research::StudySubject>,
    slabs: Query<Entity, With<StudySlab>>,
    occupants: Query<Entity, With<SlabOccupant>>,
) {
    let wanted = subject.0.is_some();
    let present = occupants.iter().next().is_some();
    if wanted == present {
        return; // already agrees with the research state
    }
    if !wanted {
        for e in &occupants {
            commands.entity(e).despawn();
        }
        return;
    }
    for slab in &slabs {
        commands.entity(slab).with_child((
            SlabOccupant,
            SiteVisual,
            // The same stand-in a containment cell uses, and for the same reason: `Specimen` records
            // only `captured: Entity` and that entity dies with the expedition, so the Site genuinely
            // does not know the species. Lying down rather than standing — it is on a table.
            Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::splat(0.45)),
            Visibility::Inherited,
            WorldAssetRoot(assets.load(
                GltfAssetLabel::Scene(0).from_asset(kit.glb(SitePiece::SpecimenStandin).to_owned()),
            )),
        ));
    }
}

/// Ease each operative's animation blend from how far its avatar actually moved this frame.
///
/// **Cosmetic, `Update` only**, and it writes nothing but a `PoseBlender` — so it cannot reach
/// `snapshot_hash`, exactly as `docs/animation.md` requires of the whole animation layer.
///
/// Simpler than `squad::drive_valkyrie_animation` because a hub avatar is simpler: `drive_avatars`
/// turns it to face the way it actually moved, so travel is always straight ahead in its own frame
/// (`theta = 0`), and nobody aims or fires in the Site. What is left is idle ↔ walk ↔ run, which is
/// the whole of what a hub needs.
fn drive_avatar_animation(
    time: Res<Time>,
    avatars: Query<&Transform, With<SiteAvatar>>,
    mut models: Query<(&ChildOf, &mut anim::PoseBlender, &mut AvatarLoco), With<AvatarModel>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    // Same exponential form the squad's smoothing uses — frame-rate independent, so the blend looks
    // the same at 30 and 240 fps.
    let ease = 1.0 - (-dt / AVATAR_LOCO_TAU).exp();
    for (child_of, mut blender, mut loco) in &mut models {
        let Ok(tf) = avatars.get(child_of.parent()) else {
            continue; // parent is not an avatar (or was despawned)
        };
        let raw = match loco.last {
            // First frame: no previous position, so no speed can be measured yet. Starting at 0 shows
            // the idle clip, which is the correct pose for an operative that has not been ordered.
            None => 0.0,
            Some(prev) => (tf.translation - prev).with_y(0.0).length() / dt,
        };
        loco.last = Some(tf.translation);
        loco.speed += (raw - loco.speed) * ease;
        let weights = crate::squad::valkyrie_weights(loco.speed, 0.0, false, false);
        if let Err(e) = blender.set_targets(&weights) {
            error!("site avatar: {e}");
        }
        blender.set_ground_speed(loco.speed);
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
    let Some(anchor) = anchor.filter(|a| a.valid) else {
        return;
    };
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
    let Ok(roster) = rosters.get(site.0) else {
        return;
    };

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
            // Half a booth in, not at the origin: the cell entity sits exactly where the GLASS is, so
            // a body at (0,0,0) stood inside the pane it is meant to be seen through.
            Transform::from_translation(cell_interior_dir(cell.yaw) * (CELL_DEPTH * 0.5))
                .with_scale(Vec3::splat(0.6)),
            Visibility::Inherited,
            WorldAssetRoot(assets.load(
                GltfAssetLabel::Scene(0).from_asset(kit.glb(SitePiece::SpecimenStandin).to_owned()),
            )),
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
