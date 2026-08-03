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
use super::layout::{AreaId, SiteLayout};
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

/// Which `SiteLayout::props` record this body was spawned from.
///
/// Carried by dressing props only, and read by exactly one consumer: the dev-only Site editor, which
/// needs to get from a prop under the cursor back to the source line that authored it. Positions are
/// not a usable key — the shipped layout stacks four crates and lays three threshold pads in a row —
/// and `props` records carry no id, so the spawn-order index is the identity.
///
/// **Indices are only valid against the layout that spawned them.** Anything that inserts or removes
/// a record must renumber every body after it (`site_editor::edit`), or the editor starts writing to
/// the wrong line.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropIndex(pub usize);

pub use super::people::{CastId, Operative, SiteAvatar, Staff};

/// Where this avatar is walking, if anywhere.
#[derive(Component, Debug, Default)]
pub struct AvatarGoal(pub Option<Vec3>);

impl AvatarGoal {
    /// **Is this body actually walking?** — the question `Velocity` would answer for a squad unit.
    ///
    /// Site avatars deliberately carry no `Velocity`: `drive_avatars` eases `Transform` toward the
    /// goal directly, and `site::mod` gives the full reason a `SiteAvatar` is not a `squad::Unit`.
    /// So "moving" has to be asked of the goal, and it has to be asked with an epsilon — the ease is
    /// exponential, so a bare `goal.is_some()` stays true forever as the position converges and a
    /// footstep voice keyed on it would never stop.
    pub fn walking(&self, at: &Transform, epsilon: f32) -> bool {
        self.0
            .is_some_and(|g| at.translation.distance_squared(g) > epsilon * epsilon)
    }
}

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

/// The GLB child of an **operative's** [`SiteAvatar`]. Carries the cosmetic animation state, never the
/// avatar itself — the same split `squad::FigurineModel` makes, and for the same reason (issue #18).
///
/// ⚠️ **Staff models carry [`StaffModel`] instead, and the split is load-bearing.** This marker means
/// "a model whose blender holds the Valkyrie's ten slots in the Valkyrie's order", because that is what
/// [`drive_avatar_animation`] feeds it. A staff body has four slots; `PoseBlender::set_targets` refuses
/// a length mismatch by **writing nothing**, so a staff member caught by this query would hold bind
/// pose forever while logging once per frame. Two markers is what makes that untypeable.
#[derive(Component)]
struct AvatarModel;

/// The GLB child of a **staff member's** [`SiteAvatar`]. See [`AvatarModel`] for why these are two
/// markers rather than one.
#[derive(Component)]
struct StaffModel;

/// Which idle a staff body stands in.
///
/// Constant per person, chosen from a stable hash of their `CastId` at spawn. Nine bodies all playing
/// clip 0 breathe in perfect lockstep, which the eye reads as a rendering artefact rather than as
/// people; `util::hash01_u32` exists precisely so that per-spawn variation is not keyed on position.
#[derive(Component, Debug, Clone, Copy)]
struct IdleLook(bool);

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
        // ...and run every authored prop through the SAME placement rules the dungeon's solved
        // furniture obeys. The Site is hand-authored on purpose, which is exactly why it needs this:
        // nothing else stands between a typo'd coordinate and a bunk halfway through a wall.
        //
        // Read from the kit resource rather than re-loading the file: `SitePlugin` inserts
        // `SiteKitRes` and this plugin is added after it, so the kit is the one already validated.
        match app.world().get_resource::<crate::site::SiteKitRes>() {
            Some(kit) => match super::layout::check_prop_placements(&layout, &kit.0) {
                Ok(waived) => {
                    for w in waived {
                        info!("site: prop placement {w}");
                    }
                }
                Err(e) => {
                    error!("{e}");
                    return;
                }
            },
            // The kit is inserted by `SitePlugin::build`. If it is absent the plugins were reordered,
            // and silently skipping the check is how the rule quietly stops applying — so say so.
            None => {
                error!(
                    "site: SiteKitRes is missing at SiteVisualsPlugin::build, so prop placements \
                     CANNOT be checked — Site-67 will not be built. Add SitePlugin before it."
                );
                return;
            }
        }
        // The staff, validated at the door. One path: a missing `staff.ron` means "no staff yet" and
        // is normal, but a malformed one is a loud failure and the Site does not build — exactly the
        // stance taken two blocks above for the layout and the kit. An author who mistyped a title
        // must see it, rather than walk into an empty hub and wonder where everyone went.
        let staff = match super::people::load_site_staff(super::people::STAFF_PATH) {
            Ok(s) => s,
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
            .insert_resource(SiteStaffRes(staff))
            // AFTER both graphs exist: `spawn_site_geometry` pins `ValkyrieAnim`'s graph and slots on
            // each operative's model child, and `StaffAnim`'s on each staff member's, as an
            // `anim::BlendSource`. Bevy is otherwise free to order two `Startup` systems either way
            // round. The squad states the same constraint for `spawn_unit`; the Site is the second
            // spawner and now needs it twice over.
            .add_systems(Startup, super::staff_anim::build_staff_anim)
            .add_systems(
                Startup,
                spawn_site_geometry
                    .after(crate::squad::build_valkyrie_anim)
                    .after(super::staff_anim::build_staff_anim),
            )
            // Cosmetic, so `Update` — never `FixedUpdate` (`docs/animation.md`). Deliberately NOT
            // gated on `AppState::Site`: `apply_pose_blenders` snaps weights on its first pass, so a
            // blender that had never been driven would prime to all-zero and show one frame of bind
            // pose the moment the player walks in. Five avatars is not a cost worth that.
            .add_systems(Update, (drive_avatar_animation, drive_staff_animation))
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

/// The authored staff roster, loaded once at plugin build.
///
/// A resource for the same reason `SiteKitRes` is one: it is validated at the door, and the systems
/// that need it should read the copy that was already proven good rather than re-reading the file.
#[derive(Resource, Deref)]
pub struct SiteStaffRes(pub Vec<super::people::StaffMember>);

/// One panel of perimeter wall, as a segment on the **floor's edge**.
///
/// `(x, z)` is the segment's LOW lattice endpoint and `along_x` its direction, so a panel is exactly a
/// unit edge of the floor grid: `along_x` runs from `(x, z)` to `(x+1, z)` on the line `z`, otherwise
/// from `(x, z)` to `(x, z+1)` on the line `x`. Ordered and hashable so the spawn walks them in a
/// stable order rather than a `HashSet`'s.
type WallPanel = (i32, i32, bool);

/// Every panel the perimeter needs, derived from **floor edges** rather than from wall-cell centres.
///
/// # The rule
///
/// *For every floor cell, for each of its four edges whose neighbour is a wall cell, one 1 m panel
/// centred on that edge.* That is the whole model, and everything else falls out of it.
///
/// # Why it replaced three attempts at the same bug
///
/// A wall cell is a whole 1 m cell but the panel in it is 0.10 m thick, so "where is the wall?" had no
/// answer the floor agreed with. Drawing it at the cell CENTRE put it half a cell off the floor edge;
/// the corner point then landed half a cell from where the panels actually were, and — this is the
/// part that kept biting — that offset points the *opposite way* at a convex corner than at a concave
/// one. Three fixes in a row (crossed slabs, then half-length legs, then seating the panels on the
/// floor edge) each corrected the case in front of them and broke the other: stubs jutting into open
/// space at one, and a lit cap post standing alone in a 0.5 m hole at the other.
///
/// Keying on the floor edge removes the offset instead of correcting it. The wall line and the floor
/// grid become the same line **by construction**, so panel joints land on floor seams, and a corner is
/// simply two perpendicular panels whose endpoints coincide — no gap and no overhang, at a convex
/// corner, a concave one or a T, with no piece other than the plain 1 m panel.
///
/// `site67.ron` still says which cells are wall — that is what `is_walkable` and `SiteLayout::validate`
/// read. Only the question "where is its face" is answered here.
pub(crate) fn wall_panels(l: &SiteLayout) -> std::collections::BTreeSet<WallPanel> {
    let wall_cells: std::collections::HashSet<(i32, i32)> = l
        .walls
        .iter()
        .filter(|w| w.piece == SitePiece::Wall)
        .map(|w| w.cell)
        .collect();
    let mut panels = std::collections::BTreeSet::new();
    for r in &l.floor {
        for c in r.cells() {
            let (x, z) = (c.x, c.y);
            if wall_cells.contains(&(x + 1, z)) {
                panels.insert((x + 1, z, false));
            }
            if wall_cells.contains(&(x - 1, z)) {
                panels.insert((x, z, false));
            }
            if wall_cells.contains(&(x, z + 1)) {
                panels.insert((x, z + 1, true));
            }
            if wall_cells.contains(&(x, z - 1)) {
                panels.insert((x, z, true));
            }
        }
    }
    panels
}

/// Where a panel's centre sits in world space, and the yaw that lays it along its edge.
///
/// `wall.glb` is thin along X and 1 m long along Z, so a panel running along Z is yaw 0 and one
/// running along X is yaw 90 — the same convention `site67.ron`'s walls header states.
fn panel_transform(l: &SiteLayout, (x, z, along_x): WallPanel) -> (Vec3, f32) {
    if along_x {
        (l.point((x as f32 + 0.5, z as f32)), 90.0)
    } else {
        (l.point((x as f32, z as f32 + 0.5)), 0.0)
    }
}

/// The lattice points where two **perpendicular** panels meet — the corners, derived from the panels
/// themselves rather than from anything authored.
///
/// A straight run only ever contributes panels of one direction, so it yields no corners; a corner,
/// and equally a T or a crossing, contributes both. That makes the cap reachable by construction:
/// edit the layout however you like and the corners follow.
fn corner_vertices(
    panels: &std::collections::BTreeSet<WallPanel>,
) -> std::collections::BTreeSet<(i32, i32)> {
    let mut ends_x = std::collections::HashSet::new();
    let mut ends_z = std::collections::HashSet::new();
    for &(x, z, along_x) in panels {
        if along_x {
            ends_x.insert((x, z));
            ends_x.insert((x + 1, z));
        } else {
            ends_z.insert((x, z));
            ends_z.insert((x, z + 1));
        }
    }
    ends_x.intersection(&ends_z).copied().collect()
}

/// Which way a corner's cap faces, in degrees about +Y — from the directions panels LEAVE the vertex.
///
/// **The shipped Ozea cap is a 0.22 × 0.22 square post, so this yaw is currently invisible.** It is
/// computed anyway because the greybox kit's corner is an L (`kenney .../wall-corner.glb`), and an L
/// pointed the wrong way at three of four corners is a silent, per-corner wrongness nobody would trace
/// back here. `SITE_KIT_PATH` is one line; this keeps a kit swap from needing a second one.
fn corner_yaw(panels: &std::collections::BTreeSet<WallPanel>, (x, z): (i32, i32)) -> f32 {
    let px = panels.contains(&(x, z, true));
    let nx = panels.contains(&(x - 1, z, true));
    let pz = panels.contains(&(x, z, false));
    let nz = panels.contains(&(x, z - 1, false));
    match (px, pz, nx, nz) {
        (true, true, _, _) => 0.0,
        (_, true, true, _) => 90.0,
        (_, _, true, true) => 180.0,
        (true, _, _, true) => 270.0,
        // A T or a crossing leaves in three or four directions; a square post is right there anyway.
        _ => 0.0,
    }
}

/// One GLB piece, placed. The scene rides a **cosmetic child** — the same discipline every creature
/// spawn uses, because an async scene load attaching `Children`/`SceneInstance` to an entity other
/// systems query is the archetype churn `sim_harness` was hardened against.
pub(crate) fn place(
    commands: &mut Commands,
    assets: &AssetServer,
    kit: &crate::site::kit::SiteKit,
    piece: SitePiece,
    at: Vec3,
    yaw_deg: f32,
) -> Entity {
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
                // Two different scales, and they answer different questions.
                //
                // `y_scale` is GAME POLICY stretching Y alone: a wall must reach `WALL_HEIGHT`
                // whatever the artist made it, and dressing (`target_height` → `None`) is left at the
                // size it was authored.
                //
                // `scale` is an ART CORRECTION applied uniformly: one of the libraries the dressing
                // draws on (`assets/low_poly_furniture/`) was converted for the dungeon's manifest,
                // which carries its own footprint, so nothing there ever had to be life-size — `Books
                // A.glb` measures a half-metre wide. Uniform, so it never distorts a shape, and the
                // kit's `height`/`footprint` are the post-scale values because every placement rule
                // reads them. See `KitPiece::scale`.
                .with_scale(Vec3::splat(kit.scale(piece)) * Vec3::new(1.0, kit.y_scale(piece), 1.0)),
            Visibility::Inherited,
        ))
        .with_child((WorldAssetRoot(scene), Transform::default()))
        .id()
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
    // ...and the light it throws into the hall. On the HALL side of the quad (`+normal`), because a
    // light at the quad's own plane is half-buried in the frame and spills backwards through the
    // perimeter wall. `drive_aperture_charge` breathes and charges its intensity; the value here is
    // only the first frame's.
    commands.spawn((
        SiteVisual,
        super::aperture::ApertureGlow,
        PointLight {
            color: super::aperture::APERTURE_LIGHT_TINT,
            intensity: super::aperture::APERTURE_LIGHT_LUMENS,
            // Reaches into the hall without washing the containment wing next door.
            range: 9.0,
            // The one light in the Site whose shadows are the POINT: the frame's jambs cropping the
            // spill is what makes the glow read as coming through an opening.
            shadow_maps_enabled: true,
            contact_shadows_enabled: true,
            ..default()
        },
        Transform::from_translation(frame_at + Vec3::Y * oh * 0.55 + normal * 0.55),
    ));
}

/// From a non-floor cell's CENTRE to the edge of the floor it borders.
///
/// The perimeter is drawn from floor edges (`wall_panels`), and the ASYNC doorway stands in a gap in
/// that same perimeter — so its frame has to sit on the same line the panels either side of it do,
/// not half a cell back at its own cell centre. `frame_pos` stays authored in CELL space because that
/// is what `validate_doorway_gap` checks; the step onto the edge is taken here, once, at render time.
fn floor_edge_offset(l: &SiteLayout, cell: IVec2) -> Vec3 {
    for (step, off) in [
        (IVec2::Y, Vec3::Z),
        (IVec2::NEG_Y, Vec3::NEG_Z),
        (IVec2::X, Vec3::X),
        (IVec2::NEG_X, Vec3::NEG_X),
    ] {
        if l.is_floor(cell + step) {
            return off * 0.5;
        }
    }
    Vec3::ZERO
}

/// Height of the examination slab's bed platform, in metres — where a study subject lies.
///
/// Measured off `slab.glb` (`SM_MedPod_Treatment_Bed`): the widest horizontal band of the mesh is at
/// y ≈ 0.55–0.60, which is the mattress. Not guessed, and not the mesh's 1.72 m overall height, which
/// is the raised canopy at the head end.
const SLAB_SURFACE_Y: f32 = 0.60;

/// How deep a containment booth runs behind its glazed front, in metres. Two cells, matching the 2 m
/// span of `wall_window` itself, so a cell is square in plan.
pub(crate) const CELL_DEPTH: f32 = 2.0;

/// Half the depth of a containment CELL ROOM, in metres — the 3x3 rects `site67.ron` authors.
///
/// Used to find the room's corridor-facing wall from its authored centre, so the observation window
/// follows the room rather than being a second coordinate that can disagree with it.
pub(crate) const CELL_ROOM_HALF_DEPTH: f32 = 1.5;

/// Height of the TOP plaque's base above the floor, in metres. Eye height for a standing person,
/// which is where signage that has to be read while walking belongs.
const PLAQUE_EYE_HEIGHT: f32 = 1.55;
/// Vertical pitch between stacked plaques. Slightly more than the 0.18 m sign is tall, so the stack
/// reads as separate plates rather than as one tall one — the count is the whole message.
const PLAQUE_STACK_STEP: f32 = 0.24;
/// How far along the wall from the doorway's centre the plaque hangs.
///
/// ⚠️ **Inside the doorway cell, not past it.** This was 0.62 — deliberately "past the opening's edge
/// so it is on solid wall" — and that reasoning is wrong for how the Site builds walls: a panel sits
/// on the BOUNDARY of the doorway cell, half a metre out, so anything beyond that is inside the
/// neighbouring wall cell and the sign is swallowed by the geometry. Found by rendering it and seeing
/// nothing. 0.42 hangs it in the door's own reveal, where it is visible from both approaches.
const PLAQUE_BESIDE_DOOR: f32 = 0.42;

/// Which way the wall a doorway sits in RUNS, as a unit step.
///
/// Same half-turn convention as every wall in the kit and as `SiteLayout::doorway_run_step`: a frame
/// at yaw 90 separates along X, so its wall runs along X and the plaque hangs beside it that way.
fn plaque_run(yaw_deg: f32) -> Vec3 {
    if (45.0..135.0).contains(&yaw_deg.rem_euclid(180.0)) {
        Vec3::X
    } else {
        Vec3::Z
    }
}

/// Which way a containment cell's interior lies, given its authored yaw.
///
/// `wall_window` is thin along local X and 2 m long along local Z (same convention as every wall in
/// the kit), so the glass faces `rot(yaw) · X` and the booth runs the other way.
pub(crate) fn cell_interior_dir(yaw_deg: f32) -> Vec3 {
    -(Quat::from_rotation_y(yaw_deg.to_radians()) * Vec3::X)
}

/// Build the booth behind a containment cell's glass: two side walls and a back.
///
/// **Derived from the cell's own placement, never authored** — the same discipline [`wall_panels`]
/// and [`corner_vertices`] use, so adding a seventh cell to `site67.ron` gets an enclosure for free
/// and no cell can be left as a bare pane by forgetting to type one.
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
    staff_anim: Res<super::staff_anim::StaffAnim>,
    staff: Res<SiteStaffRes>,
    nav: Res<SiteNav>,
) {
    let l = &layout.0;
    let staff = &staff.0;
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
    // THE PERIMETER, from floor edges — see `wall_panels` for why this replaced three attempts at
    // deriving it from wall-cell centres.
    let panels = wall_panels(l);
    for &panel in &panels {
        let (at, yaw) = panel_transform(l, panel);
        let e = place(&mut commands, &assets, &kit, SitePiece::Wall, at, yaw);
        // The knee-wall cutaway, which the hub went without for its whole life. A panel that encloses
        // nothing (floor on both sides, or on neither) gets no normal and is left standing — see
        // `cutaway::panel_outward`.
        if let Some(outward) = super::cutaway::panel_outward(l, panel) {
            commands.entity(e).insert(super::cutaway::SiteKneeWall {
                outward,
                base_scale_y: kit.scale(SitePiece::Wall) * kit.y_scale(SitePiece::Wall),
            });
        }
    }
    // ── DOORWAYS ─────────────────────────────────────────────────────────────────────────────────
    //
    // **Site-67 has doors now.** The old hub had none — an opening was the absence of wall, so a room
    // stood open along a whole shared edge — and that was inherited from `placement::furnish`'s
    // Backrooms art direction (*"No doors — the Backrooms look leaves every opening as a bare
    // doorway"*), which is a decision about the DUNGEON. The Director corrected it on 2026-08-02: the
    // hub is a Foundation facility, and facilities have doors.
    //
    // The frame stands ON the doorway cell, which is floor — see `layout::Doorway` for why an opening
    // has to be floor rather than a hole in the floor. The header course closes the 0.40 m band above
    // it, without which the perimeter has a slot straight through it at head height; the same fix the
    // ASYNC aperture needed on 2026-08-01.
    for d in &l.doorways {
        let at = l.cell_center(IVec2::new(d.cell.0, d.cell.1));
        place(&mut commands, &assets, &kit, SitePiece::WallDoorway, at, d.yaw);
        place(
            &mut commands,
            &assets,
            &kit,
            SitePiece::WallHeader,
            at + Vec3::Y * crate::dungeon::DOORWAY_HEIGHT,
            d.yaw,
        );
        // ── THE PLAQUE ───────────────────────────────────────────────────────────────────────────
        //
        // Derived from the doorway, never authored — the discipline `wall_panels`, `corner_vertices`,
        // `light_the_site` and `post_positions` all follow. Move a door and its sign moves with it;
        // change what it takes to pass and the sign changes on its own.
        //
        // **One plaque per clearance level, stacked.** A Level 2 door wears two and an open door wears
        // none, so how restricted a door is, is countable from across the corridor. Deliberately not
        // colour-coded however much SCP:CB's are — see `SitePiece::DoorPlaque`.
        let Some(level) = d.clearance else { continue };
        let along = plaque_run(d.yaw);
        for i in 0..level.rank() {
            let y = PLAQUE_EYE_HEIGHT - i as f32 * PLAQUE_STACK_STEP;
            place(
                &mut commands,
                &assets,
                &kit,
                SitePiece::DoorPlaque,
                at + along * PLAQUE_BESIDE_DOOR + Vec3::Y * y,
                d.yaw,
            );
        }
    }

    // Anything the layout puts on a wall cell that is NOT a plain wall (a column standing in a run,
    // say) still stands where it was authored: it is furniture on a cell, not a face on an edge.
    for w in &l.walls {
        if w.piece == SitePiece::Wall {
            continue;
        }
        let at = l.cell_center(IVec2::new(w.cell.0, w.cell.1));
        place(&mut commands, &assets, &kit, w.piece, at, w.yaw);
    }
    // Cap each corner, at the lattice point where two perpendicular panels already meet.
    let corners = corner_vertices(&panels);
    for &v in &corners {
        let at = l.point((v.0 as f32, v.1 as f32));
        let e = place(
            &mut commands,
            &assets,
            &kit,
            SitePiece::WallCorner,
            at,
            corner_yaw(&panels, v),
        );
        if let Some(outward) = super::cutaway::corner_outward(l, &panels, v) {
            commands.entity(e).insert(super::cutaway::SiteKneeWall {
                outward,
                base_scale_y: kit.scale(SitePiece::WallCorner)
                    * kit.y_scale(SitePiece::WallCorner),
            });
        }
    }
    for (ix, p) in l.props.iter().enumerate() {
        let mut at = l.point(p.pos);
        // A dressing prop that rests on a surface takes its height from the host it stands on —
        // derived, never authored (`kit::KitPiece::rests_on`). `check_prop_placements` has already
        // refused any resting prop with no host, so an `Err` here cannot reach a built Site; it is
        // logged rather than ignored so a future caller that skipped the check is not silent.
        if let Some(rest) = super::layout::resting_on(l, &kit, p) {
            match rest {
                Ok((top, _host_ix)) => at.y += top,
                Err(e) => warn!("site: {e}"),
            }
        }
        let e = place(&mut commands, &assets, &kit, p.piece, at, p.yaw);
        // Which `site67.ron` record this body came from. The dev-only Site editor (F7) needs a way
        // back from a prop the cursor is over to the line that authored it, and an index carried at
        // spawn is the only honest answer — positions are not unique (four crates share a stack) and
        // `props` records have no id of their own.
        //
        // Inert data on a body that carries no `Health`, so it contributes nothing to
        // `sim_harness::snapshot_hash` and is invisible to the deterministic core. Unconditional, not
        // `#[cfg(debug_assertions)]`, so release and debug spawn the same archetype — a marker that
        // splits the archetype only in one build is the shape `dialogue::ensure_leader` documents as a
        // determinism hazard.
        commands.entity(e).insert(PropIndex(ix));
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
    //
    // The frame seats on the run's line like every other panel does, so it does not stand half a cell
    // proud of the wall it fills. `frame_pos` stays authored in CELL space — that is what
    // `validate_doorway_gap` checks — and the seat is applied here, at render time, exactly once.
    let door_cell = IVec2::new(
        l.door.frame_pos.0.floor() as i32,
        l.door.frame_pos.1.floor() as i32,
    );
    let door_seat = floor_edge_offset(l, door_cell);
    let frame_at = l.point(l.door.frame_pos) + door_seat;
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
        let at = l.cell_center(cell) + door_seat + Vec3::Y * crate::dungeon::DOORWAY_HEIGHT;
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
    // **A cell is a ROOM now, so it needs no booth.** `enclose_containment_cell` used to build two
    // side walls and a back behind each pane, because a cell was a 2 m alcove standing on the open
    // deck of the containment wing. The twelve cells are authored rects in `areas:`/`floor:` as of
    // 2026-08-02, so the perimeter pass walls them like every other room and building a second
    // enclosure inside the first would put two walls in one place.
    //
    // What survives is the OBSERVATION WINDOW: a cell you can only see into by opening its door is a
    // cupboard, and the containment wing's whole job is to be a rack of held things you walk past.
    // The window goes in the corridor-facing wall beside the door — `window_offset` is one cell to the
    // side of the doorway, which is the only part of that wall guaranteed to be solid.
    for c in &l.cells {
        let at = l.point(c.pos);
        // The wall between this cell and the cell row: half the room's depth from its centre, on the
        // side its `yaw` faces. Derived from the authored centre, never a second authored number.
        let facing = Quat::from_rotation_y(c.yaw.to_radians()) * Vec3::Z;
        let window_at = at + facing * (CELL_ROOM_HALF_DEPTH) + facing.cross(Vec3::Y) * 1.0;
        place(
            &mut commands,
            &assets,
            &kit,
            SitePiece::WallWindow,
            window_at,
            c.yaw + 90.0,
        );
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
            SiteAvatar,
            Operative(i),
            CastId::of_operative(i),
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

    // The staff. Nine people who work here, posted to the rooms they work in.
    //
    // Grouped by post first, because the spawn point is DERIVED rather than authored: everyone sharing
    // a room is placed together so `post_positions` can space them out and keep them clear of the
    // furniture. Iterating the roster in order and asking for one cell at a time would stand the second
    // cook exactly where the first one already is.
    //
    // The grouping walks `AreaId::REQUIRED` rather than a map of whatever posts happen to appear, so
    // the iteration order is a fixed compile-time list and not a hash order.
    let mut staffed = 0usize;
    for area in super::layout::AreaId::REQUIRED {
        let here: Vec<(usize, &super::people::StaffMember)> = staff
            .iter()
            .enumerate()
            .filter(|(_, s)| s.post == *area)
            .collect();
        if here.is_empty() {
            continue;
        }
        let spots = super::people::post_positions(l, &kit.0, &nav, *area, here.len());
        for ((index, member), spot) in here.iter().zip(spots.iter()) {
            let at = l.point((spot.x, spot.y));
            let cast = CastId::of_staff(*index);
            let rig = member.rig;
            commands
                .spawn((
                    SiteVisual,
                    SiteAvatar,
                    Staff(*index),
                    cast,
                    AvatarGoal::default(),
                    Transform::from_translation(at),
                    Visibility::Inherited,
                ))
                .with_child((
                    StaffModel,
                    // Constant per person and stable across boots — see `IdleLook`.
                    IdleLook(crate::util::hash01_u32(cast.0 as u32) < 0.5),
                    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(rig.glb()))),
                    // Same authored scale and the same half-turn as the operatives: these rigs share
                    // the Valkyrie's MPFB2 lineage and face glTF +Z, so an unrotated body would stand
                    // with its back to the camera.
                    Transform::from_scale(Vec3::splat(FIGURINE_SCALE))
                        .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
                    anim::BlendSource {
                        graph: staff_anim.get(rig).graph.clone(),
                        slots: staff_anim.get(rig).slots.clone(),
                    },
                    AvatarLoco::default(),
                ));
            staffed += 1;
        }
    }
    if staffed != staff.len() {
        // `post_positions` already warns per person; this is the count, so a roster that half-spawned
        // is visible in one line rather than reconstructed from scattered warnings.
        warn!(
            "site: {staffed} of {} staff were placed — the roster and the layout disagree",
            staff.len()
        );
    }

    // The panel and corner counts are how the derived rule is verified at runtime — `site67.ron`
    // authors no panels and no corners, so a 0 in either means the derivation stopped matching the
    // layout it is supposed to trace.
    info!(
        "site: built Site-67 ({} floor runs, {} cells, {} wall panels, {} corners, {} operatives, \
         {staffed} staff)",
        l.floor.len(),
        l.cells.len(),
        panels.len(),
        corners.len(),
        l.spawns.len()
    );
}

fn focus_camera_on_site(
    layout: Res<SiteLayoutRes>,
    mut rig: ResMut<crate::camera::CameraRig>,
    mut cams: Query<&mut Transform, With<crate::MainCamera>>,
) {
    // Aim at the spine's middle so all six areas are within a short pan.
    let l = &layout.0;
    let focus = l.cell_center(IVec2::new(16, 13));
    crate::camera::snap_camera_to(focus, &mut rig, &mut cams);
}

/// Ceiling height for the Site's own fixtures — just above `WALL_HEIGHT` so a light hangs over the
/// wall tops rather than inside them.
const SITE_FIXTURE_Y: f32 = 2.6;

/// Grid spacing (metres) between fixtures within an area.
///
/// **Deliberately wider than [`AreaLight::range`] now.** It used to equal the range exactly, with the
/// stated goal that "pools overlap slightly instead of leaving dark bands between them" — which is a
/// description of even fill, and even fill is what made the hub read as a floorplan rather than a
/// building. Darkness between pools is what makes architecture legible; the gap is the point.
const SITE_FIXTURE_SPACING: f32 = 11.0;

/// One area's lighting character. **Per-wing colour is the wayfinding**, not just the mood: the design
/// doc (§2.1) makes "learnable without signage" the hub's whole reason for being hand-authored, and
/// until 2026-08-02 the entire burden of that fell on a single floor decal per room while every
/// fixture in the building emitted the same near-white.
///
/// `kelvin` is a real colour temperature run through [`blackbody_srgb`] rather than a hand-picked RGB,
/// so the wings sit on one physical scale and cannot drift into arbitrary hues relative to each other.
struct AreaLight {
    /// Correlated colour temperature. Warm = lived in, cool = clinical.
    kelvin: f32,
    /// Luminous power of the area's KEY fixture — the one nearest its centre, and the only one that
    /// casts a real shadow map.
    key_lumens: f32,
    /// Luminous power of the remaining FILL fixtures.
    fill_lumens: f32,
    /// Per-fixture reach in metres. Shorter than [`SITE_FIXTURE_SPACING`] leaves the pools separate.
    range: f32,
}

/// How each wing is lit, and why.
///
/// The temperatures are the Director's call (2026-08-02). They read as a sequence when you walk the
/// spine, which is the point: you can tell which wing you are in from the colour of the air before you
/// can read the decal on the floor.
fn area_light(id: super::layout::AreaId) -> AreaLight {
    use super::layout::AreaId::*;
    match id {
        // **A cell.** Colder and harder than the corridor outside it, and deliberately over-lit for
        // its size: this is a room designed so that nothing in it is ever in shadow, which is what a
        // containment cell is FOR. Short range because it is 3x3 — a 10 m falloff in a 3 m room just
        // spills through the doorway and lights the corridor twice.
        ContainmentCell => AreaLight {
            kelvin: 6900.0,
            key_lumens: 300_000.0,
            fill_lumens: 210_000.0,
            range: 5.0,
        },
        // Clinical and high-key. This is the one room that must look like it is inspected daily.
        Containment => AreaLight {
            kelvin: 6500.0,
            key_lumens: 380_000.0,
            fill_lumens: 260_000.0,
            range: 10.0,
        },
        // Surgical, and mostly dark except where it matters — the slab gets its own spot below, so the
        // ambient here is deliberately thin. A bright empty room reads as an empty room.
        Research => AreaLight {
            kelvin: 5600.0,
            key_lumens: 230_000.0,
            fill_lumens: 150_000.0,
            range: 9.5,
        },
        // Tungsten, low and pooled. Paper, dust, and a desk lamp's worth of light.
        Records => AreaLight {
            kelvin: 2900.0,
            key_lumens: 220_000.0,
            fill_lumens: 145_000.0,
            range: 9.0,
        },
        // Sodium. A loading bay, lit for work rather than for comfort.
        Requisition => AreaLight {
            kelvin: 2400.0,
            key_lumens: 240_000.0,
            fill_lumens: 155_000.0,
            range: 9.5,
        },
        // Warm. The room you leave from, and the one place in the Site that should feel like people.
        Briefing => AreaLight {
            kelvin: 3200.0,
            key_lumens: 265_000.0,
            fill_lumens: 175_000.0,
            range: 9.5,
        },
        // Neutral connective tissue, and the DIMMEST thing in the building. A corridor you walk along
        // between brighter rooms is what makes the rooms read as rooms.
        Corridor => AreaLight {
            kelvin: 4000.0,
            key_lumens: 150_000.0,
            fill_lumens: 115_000.0,
            range: 9.0,
        },
        // ⚠️ NOT in the Director's table, which named the six destinations and the spine. Chosen here,
        // and stated rather than buried: the ASYNC hall is lit dim and neutral **so the aperture is the
        // brightest thing in it**. Lighting this room properly would put a sodium portal in a well-lit
        // hall and lose it; the hall's job is to be the dark room the door glows into.
        AsyncDoor => AreaLight {
            kelvin: 4000.0,
            key_lumens: 110_000.0,
            fill_lumens:  75_000.0,
            range: 9.0,
        },

        // ── The living half (2026-08-02) ──
        //
        // These five are lit to say "people are here", which mostly means WARMER and, in one case,
        // DIMMER than any working room. The point of walking from containment's 6500 K into quarters'
        // 2400 K is that the second one feels like somewhere you would take your boots off.

        // The dimmest room in the Site after the ASYNC hall, and the warmest anywhere. A bunk room lit
        // to working brightness is a barracks inspection, not somewhere anyone sleeps. This is the
        // SECOND deliberate exception to "every room out-keys the spine" — see the lighting test.
        Quarters => AreaLight {
            kelvin: 2400.0,
            key_lumens: 105_000.0,
            fill_lumens: 62_000.0,
            range: 8.0,
        },
        // Galley: warm, but bright enough to work in — between the quarters and a workroom, which is
        // exactly what a mess is.
        Kitchen => AreaLight {
            kelvin: 3000.0,
            key_lumens: 210_000.0,
            fill_lumens: 130_000.0,
            range: 8.5,
        },
        // Training and recreation. Brighter and less warm than the galley: you can read a board or spot
        // someone lifting, but it is still not a laboratory.
        Activities => AreaLight {
            kelvin: 3500.0,
            key_lumens: 225_000.0,
            fill_lumens: 140_000.0,
            range: 8.5,
        },
        // Planning light: neutral and even, because this is the one room where the player is reading a
        // map, and a colour cast would lie about what is on it.
        WarRoom => AreaLight {
            kelvin: 4500.0,
            key_lumens: 240_000.0,
            fill_lumens: 150_000.0,
            range: 8.5,
        },
        // Console light — cool, and deliberately a shade below containment's 6500 K, which it stands
        // beside and watches, so the boundary between them still reads rather than merging into one
        // continuous white space.
        Monitoring => AreaLight {
            kelvin: 6000.0,
            key_lumens: 200_000.0,
            fill_lumens: 125_000.0,
            range: 8.5,
        },
    }
}

/// Luminous power of the surgical spot over the examination slab, in lumens. The brightest single
/// fixture in the Site by a wide margin — it is the one pool of light the research wing is *about*.
const SLAB_SPOT_LUMENS: f32 = 420_000.0;

/// Height of the slab spot above the floor. Below the ceiling fixtures, so it reads as a task light
/// swung down over the table rather than as room lighting.
const SLAB_SPOT_Y: f32 = 2.15;

/// Convert a correlated colour temperature to a **linear** sRGB multiplier.
///
/// Kim et al. (2002), "Design of Advanced Color Temperature Control System for HDTV Applications",
/// *J. Korean Phys. Soc.* 41(6) — the standard piecewise-cubic fit to the Planckian locus in CIE 1931
/// `xy`, valid over 1667–25000 K, which is the approximation graphics code has used for two decades in
/// place of integrating Planck's law against the CIE colour-matching functions. Chromaticity is then
/// taken to XYZ at `Y = 1` and through the sRGB D65 matrix (IEC 61966-2-1).
///
/// **Normalised to a peak of 1**, deliberately: the result is a *hue*, and brightness stays where it
/// belongs, in the fixture's lumens. Without that, picking a warmer wing would silently dim it.
///
/// Returns linear (not gamma-encoded) values because `PointLight::color` is a linear radiometric
/// multiplier — feeding it `Color::srgb` would apply the transfer function a second time and wash
/// every temperature toward white.
// The coefficients below are transcribed VERBATIM from Kim et al. (2002), and several carry more
// digits than `f32` can represent — which is exactly what `excessive_precision` fires on. They stay at
// full published precision anyway, because the point of a cited constant is that a reader can check it
// against the paper. Rounding them to f32's ~7 significant digits would silently make this table
// something you can no longer verify, to save nothing: the compiler rounds them identically either way.
#[allow(clippy::excessive_precision)]
fn blackbody_srgb(kelvin: f32) -> Color {
    let t = kelvin.clamp(1667.0, 25000.0);
    let (t2, t3) = (t * t, t * t * t);
    // Chromaticity x, in two pieces about 4000 K.
    let x = if t <= 4000.0 {
        -0.2661239e9 / t3 - 0.2343589e6 / t2 + 0.8776956e3 / t + 0.179910
    } else {
        -3.0258469e9 / t3 + 2.1070379e6 / t2 + 0.2226347e3 / t + 0.240390
    };
    let (x2, x3) = (x * x, x * x * x);
    // ...and y, in three, hinged on the SAME x rather than on t.
    let y = if t <= 2222.0 {
        -1.1063814 * x3 - 1.34811020 * x2 + 2.18555832 * x - 0.20219683
    } else if t <= 4000.0 {
        -0.9549476 * x3 - 1.37418593 * x2 + 2.09137015 * x - 0.16748867
    } else {
        3.0817580 * x3 - 5.87338670 * x2 + 3.75112997 * x - 0.37001483
    };
    // `y` is a denominator below. The fit cannot return zero anywhere in the clamped range, but the
    // division is guarded rather than assumed — a NaN here would blacken a whole wing silently.
    if y.abs() < 1e-6 {
        return Color::WHITE;
    }
    let (big_x, big_y, big_z) = (x / y, 1.0, (1.0 - x - y) / y);
    let r = 3.2404542 * big_x - 1.5371385 * big_y - 0.4985314 * big_z;
    let g = -0.9692660 * big_x + 1.8760108 * big_y + 0.0415560 * big_z;
    let b = 0.0556434 * big_x - 0.2040259 * big_y + 1.0572252 * big_z;
    // Out-of-gamut components come back negative; clamp, then normalise to a peak of 1.
    let (r, g, b) = (r.max(0.0), g.max(0.0), b.max(0.0));
    let peak = r.max(g).max(b);
    if peak <= 1e-6 {
        return Color::WHITE;
    }
    Color::linear_rgb(r / peak, g / peak, b / peak)
}

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
/// **Colour is the deliberate contrast, and as of 2026-08-02 it is also the wayfinding.** The dungeon's
/// fixtures are `(0.92, 1.0, 0.94)` — a faint green cast, low-CRI halophosphate, chosen to feel sickly.
/// The Site answers it per wing, on a real colour-temperature scale ([`area_light`], [`blackbody_srgb`]):
/// 6500 K in containment down to 2400 K in requisition. Before this every fixture in the building was
/// the same near-white, so light did no storytelling and no wayfinding at all, and the whole burden of
/// "which wing is this" fell on one floor decal per room.
///
/// **Key and fill, not a flat grid.** Each area's centremost fixture is its key: brighter, and the only
/// one carrying a real shadow map. The rest are fill. Every fixture gets `contact_shadows_enabled` —
/// screen-space, so it costs no shadow map — because the thing that made the hub read as a floorplan
/// was that nothing touched the floor. A prop with no contact shadow is a decal.
///
/// **Shadow budget: one shadow-mapping light per area, twelve in total.** A point light's shadow map is
/// six faces, so enabling it on all ~29 fixtures would be a very different bill. `light::
/// spawn_fixture_lights` still sets `shadow_maps_enabled: false` on every dungeon fixture — but those
/// spawn per revealed room and can reach hundreds, whereas the Site's set is fixed and small enough to
/// afford the seven that carry the whole look.
fn light_the_site(commands: &mut Commands, l: &SiteLayout) {
    for area in &l.areas {
        let r = &area.rect;
        // ⚠️ **A THRESHOLD GETS NO FIXTURE.** A doorway is a 1x1 area — it exists so that `area_at`
        // is total over walkable floor and the sign above it can name what is beyond — and it is lit
        // perfectly well by the rooms either side of it.
        //
        // This is the regression that took the Site to 5 fps and then lost the GPU device on
        // 2026-08-02. Tripling the hub took it from 12 areas to 58, of which 30 are thresholds, and
        // "one key light per area" quietly meant 58 SHADOW-CASTING point lights — 71-80 of them
        // visible at once, at 1.5 M triangles. The per-area rule was fine at twelve areas and is a
        // trap at fifty-eight: a rule that scales with a count nobody was watching.
        if r.w <= 1 && r.h <= 1 {
            continue;
        }
        let spec = area_light(area.id);
        let color = blackbody_srgb(spec.kelvin);
        // At least one fixture per area however small, then one per `SPACING` in each axis.
        let nx = ((r.w as f32) / SITE_FIXTURE_SPACING).ceil().max(1.0) as i32;
        let nz = ((r.h as f32) / SITE_FIXTURE_SPACING).ceil().max(1.0) as i32;
        // The key is the fixture nearest the middle of the grid, so it lands in the room's centre
        // rather than in a corner. Integer division floors, which is what centres an odd count and
        // picks the lower-middle of an even one — either is the middle of the room.
        let (kx, kz) = (nx / 2, nz / 2);
        for ix in 0..nx {
            for iz in 0..nz {
                // Centre each fixture in its share of the rect rather than on a corner, so an area
                // narrower than one spacing step still gets lit down its middle.
                let fx = r.x as f32 + (ix as f32 + 0.5) * (r.w as f32 / nx as f32);
                let fz = r.z as f32 + (iz as f32 + 0.5) * (r.h as f32 / nz as f32);
                let at = l.point((fx, fz)) + Vec3::Y * SITE_FIXTURE_Y;
                let is_key = ix == kx && iz == kz;
                // **Shadows only where a destination earns them.** A point light's shadow is six
                // cubemap faces, and the dungeon ships `shadow_maps_enabled: false` on every fixture
                // it has. The Site can afford them in the ten rooms the player stops in; it cannot
                // afford one per corridor run and one per cell, which is what "one per area" became.
                let casts = is_key && !matches!(area.id, AreaId::Corridor | AreaId::ContainmentCell);
                commands.spawn((
                    SiteVisual,
                    PointLight {
                        color,
                        intensity: if is_key {
                            spec.key_lumens
                        } else {
                            spec.fill_lumens
                        },
                        range: spec.range,
                        shadow_maps_enabled: casts,
                        // Contact shadows on the KEY only. This was on for every fixture, which is a
                        // per-light screen-space pass — affordable across twelve areas and not across
                        // ninety-one lights. The fills exist to lift the floor, and a fill that casts
                        // is a fill you can see doing it.
                        contact_shadows_enabled: is_key,
                        ..default()
                    },
                    Transform::from_translation(at),
                ));
            }
        }
    }
    // The surgical spot over the examination slab. A `SpotLight` rather than another point fixture
    // because the brief is "one bright pool" — a cone aimed straight down puts a hard-edged disc on the
    // table and leaves the rest of the wing dim, which is the whole composition of the room.
    //
    // Placed from the authored `Slab` prop, so moving the slab in `site67.ron` moves its light. A wing
    // with no slab simply gets no spot; nothing here assumes one exists.
    for p in l.props.iter().filter(|p| p.piece == SitePiece::Slab) {
        let at = l.point(p.pos) + Vec3::Y * SLAB_SPOT_Y;
        commands.spawn((
            SiteVisual,
            SpotLight {
                color: blackbody_srgb(area_light(super::layout::AreaId::Research).kelvin),
                intensity: SLAB_SPOT_LUMENS,
                range: 6.0,
                // Tight inner cone with a soft outer falloff: a hard disc with a legible edge, not a
                // wash. `outer` must exceed `inner` or the penumbra inverts.
                inner_angle: 0.30,
                outer_angle: 0.55,
                shadow_maps_enabled: true,
                contact_shadows_enabled: true,
                ..default()
            },
            // Straight down. `SpotLight` points along its own -Z, so a -90° pitch aims it at the floor.
            Transform::from_translation(at)
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        ));
    }
}

/// Left-click sets the player avatar's destination — the same verb the expedition uses for move orders,
/// so the hub needs no new control to learn.
fn command_avatar(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::MainCamera>>,
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

/// Measure how far a model's avatar parent moved this frame, smoothed.
///
/// Shared by both animation drivers below, which differ only in the weight vector they then produce.
/// Site avatars are moved by writing `Transform` directly (`drive_avatars`), so unlike a squad `Unit`
/// there is no `Velocity` to read and speed has to come from the position delta.
///
/// Returns `None` when the parent is not an avatar (or was despawned), which is the caller's signal to
/// skip rather than to pose.
fn measure_avatar_speed(
    tf: &Transform,
    loco: &mut AvatarLoco,
    dt: f32,
    ease: f32,
) -> f32 {
    let raw = match loco.last {
        // First frame: no previous position, so no speed can be measured yet. Starting at 0 shows the
        // idle clip, which is the correct pose for a body that has not been ordered anywhere.
        None => 0.0,
        Some(prev) => (tf.translation - prev).with_y(0.0).length() / dt,
    };
    loco.last = Some(tf.translation);
    loco.speed += (raw - loco.speed) * ease;
    let _ = dt;
    loco.speed
}

/// Ease each **operative's** animation blend from how far its avatar actually moved this frame.
///
/// **Cosmetic, `Update` only**, and it writes nothing but a `PoseBlender` — so it cannot reach
/// `snapshot_hash`, exactly as `docs/animation.md` requires of the whole animation layer.
///
/// Simpler than `squad::drive_valkyrie_animation` because a hub avatar is simpler: `drive_avatars`
/// turns it to face the way it actually moved, so travel is always straight ahead in its own frame
/// (`theta = 0`), and nobody aims or fires in the Site. What is left is idle ↔ walk ↔ run, which is
/// the whole of what a hub needs.
///
/// ⚠️ **`With<AvatarModel>` is the load-bearing half of the query.** This feeds a 10-wide vector in the
/// Valkyrie's slot order; a staff model's blender holds four. `PoseBlender::set_targets` refuses a
/// length mismatch by writing **nothing at all**, so a staff body caught here would hold bind pose for
/// the rest of the process while logging once per frame — a failure that looks like a broken asset
/// rather than a mis-typed query. See [`drive_staff_animation`].
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
        let speed = measure_avatar_speed(tf, &mut loco, dt, ease);
        let weights = crate::squad::valkyrie_weights(speed, 0.0, false, false);
        if let Err(e) = blender.set_targets(&weights) {
            error!("site avatar: {e}");
        }
        blender.set_ground_speed(speed);
    }
}

/// The same, for **staff**, whose blenders hold four slots rather than the Valkyrie's ten.
///
/// Split from [`drive_avatar_animation`] rather than branching inside it, because the difference is
/// the *shape of the weight vector* and a branch would put two incompatible contracts behind one
/// query. Two markers make the mismatch untypeable instead of merely unlikely.
///
/// Cosmetic, `Update` only, writes nothing but a `PoseBlender` — the same exemption the rest of the
/// animation layer takes.
fn drive_staff_animation(
    time: Res<Time>,
    avatars: Query<&Transform, With<SiteAvatar>>,
    mut models: Query<
        (&ChildOf, &IdleLook, &mut anim::PoseBlender, &mut AvatarLoco),
        With<StaffModel>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let ease = 1.0 - (-dt / AVATAR_LOCO_TAU).exp();
    for (child_of, look, mut blender, mut loco) in &mut models {
        let Ok(tf) = avatars.get(child_of.parent()) else {
            continue;
        };
        let speed = measure_avatar_speed(tf, &mut loco, dt, ease);
        let weights = super::staff_anim::staff_weights(speed, look.0);
        if let Err(e) = blender.set_targets(&weights) {
            error!("site staff: {e}");
        }
        blender.set_ground_speed(speed);
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
    mut cams: Query<&mut Transform, With<crate::MainCamera>>,
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
    // `With<PlayerAvatar>`, NOT `With<SiteAvatar>`. Leaving on an expedition is a decision the player
    // makes, so only the body the player drives may make it.
    //
    // This was `With<SiteAvatar>` and behaved identically, because `command_avatar` only ever moves the
    // player's avatar and the other four have never taken a step. That is about to stop being true: the
    // Site is being staffed with researchers and engineers who walk to their posts, and the ASYNC hall
    // is a room on the way to other rooms. Under the old query a cook crossing the hall would have
    // begun an expedition. Narrowed here, while it is still a latent bug rather than a live one.
    avatars: Query<&Transform, With<PlayerAvatar>>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::color::LinearRgba;

    /// Unpack the linear components a fixture will actually be given.
    fn lin(c: Color) -> LinearRgba {
        LinearRgba::from(c)
    }

    /// The Planckian fit behaves like a blackbody, not like a lookup table someone typed.
    ///
    /// This is the one piece of real physics in the Site's presentation, and it is exactly the kind of
    /// thing that goes wrong silently: a transposed coefficient still returns a plausible-looking
    /// colour, and nobody notices that requisition is faintly blue until they stand in it.
    #[test]
    fn colour_temperature_runs_warm_to_cool_in_the_right_direction() {
        // Every wing the Site actually uses, coldest first.
        let cold = lin(blackbody_srgb(6500.0));
        let mid = lin(blackbody_srgb(4000.0));
        let warm = lin(blackbody_srgb(2400.0));

        // Warm light is red-dominant and blue-starved; cool light is not. The classic sign error
        // (using `t` where the fit wants `x`) inverts precisely this.
        assert!(
            warm.red > warm.green && warm.green > warm.blue,
            "2400 K must fall off R > G > B, got {warm:?}"
        );
        assert!(
            cold.blue > warm.blue,
            "6500 K must carry more blue than 2400 K: {} vs {}",
            cold.blue,
            warm.blue
        );
        // Blue rises monotonically with temperature across the range the Site spans.
        assert!(
            warm.blue < mid.blue && mid.blue < cold.blue,
            "blue must increase with kelvin: {} {} {}",
            warm.blue,
            mid.blue,
            cold.blue
        );
    }

    /// Colour is a HUE, and brightness lives in the fixture's lumens.
    ///
    /// Without the peak normalisation, picking a warmer wing would silently dim it — the 2400 K
    /// requisition would come out darker than the 6500 K containment at identical lumens, and the
    /// difference would read as a lighting bug rather than a colour choice.
    #[test]
    fn every_temperature_is_normalised_to_the_same_peak() {
        for k in [1667.0, 2400.0, 2900.0, 3200.0, 4000.0, 5600.0, 6500.0, 25000.0] {
            let c = lin(blackbody_srgb(k));
            let peak = c.red.max(c.green).max(c.blue);
            assert!(
                (peak - 1.0).abs() < 1e-4,
                "{k} K peaks at {peak}, so it would be dimmer or brighter than its lumens say"
            );
            assert!(
                c.red >= 0.0 && c.green >= 0.0 && c.blue >= 0.0,
                "{k} K produced a negative component: {c:?}"
            );
            assert!(
                c.red.is_finite() && c.green.is_finite() && c.blue.is_finite(),
                "{k} K produced a non-finite component: {c:?}"
            );
        }
    }

    /// Out-of-range input is clamped rather than extrapolated. The fit is only valid over
    /// 1667–25000 K; outside it the cubics diverge fast, and a NaN would blacken a whole wing.
    #[test]
    fn absurd_temperatures_are_clamped_not_extrapolated() {
        assert_eq!(lin(blackbody_srgb(0.0)), lin(blackbody_srgb(1667.0)));
        assert_eq!(lin(blackbody_srgb(-5.0)), lin(blackbody_srgb(1667.0)));
        assert_eq!(lin(blackbody_srgb(1.0e9)), lin(blackbody_srgb(25000.0)));
    }

    /// The corridor is dimmer than the rooms it serves — **except the ASYNC hall, which is dimmer
    /// still, on purpose.**
    ///
    /// The exception is the whole composition of that room: the aperture is the brightest thing in it
    /// and its own `ApertureGlow` is what lights the floor, so ceiling fixtures bright enough to be
    /// comfortable would drown the portal. Every other destination is brighter than the connective
    /// tissue between them, which is what makes a room read as a room when you walk into it.
    ///
    /// `area_light` matches exhaustively so a new `AreaId` is a compile error rather than an unlit
    /// wing — this pins the part the compiler cannot: that the numbers say what the design says.
    ///
    /// (Written first as a blanket "every room beats the spine", which failed on exactly the room the
    /// exception exists for. The rule below is the one the design actually holds.)
    ///
    /// **Two rooms are darker than the corridor, for opposite reasons.** The ASYNC hall is dark so the
    /// aperture is the brightest thing in it. The quarters are dark because people sleep there. Both
    /// are listed explicitly rather than allowed by a `<=`, so adding a third dim room is a decision
    /// someone has to make here rather than something that slips through.
    #[test]
    fn the_spine_is_dimmer_than_the_working_rooms_and_brighter_than_the_dark_two() {
        use super::super::layout::AreaId;
        let spine = area_light(AreaId::Corridor);
        // Deliberately dimmer than the connective tissue, each for a stated reason.
        const DARK_ON_PURPOSE: &[AreaId] = &[AreaId::AsyncDoor, AreaId::Quarters];
        for id in AreaId::REQUIRED {
            let room = area_light(*id);
            if DARK_ON_PURPOSE.contains(id) {
                assert!(
                    room.key_lumens < spine.key_lumens,
                    "{id:?} is on the dark-on-purpose list but keys at {} against the spine's {} — \
                     either light it or take it off the list",
                    room.key_lumens,
                    spine.key_lumens
                );
                continue;
            }
            assert!(
                room.key_lumens >= spine.key_lumens,
                "{id:?} keys at {} lumens, dimmer than the corridor's {} — a corridor brighter than \
                 the rooms it serves flattens the whole hub",
                room.key_lumens,
                spine.key_lumens
            );
        }
        // Containment is the high-key room; requisition is warmer than records.
        assert!(area_light(AreaId::Containment).kelvin > area_light(AreaId::Briefing).kelvin);
        assert!(area_light(AreaId::Requisition).kelvin < area_light(AreaId::Records).kelvin);
        // The living half reads warm against the working half. Quarters is the warmest room in the
        // Site and monitoring the coolest after containment — that spread IS the wayfinding.
        assert!(
            area_light(AreaId::Quarters).kelvin <= area_light(AreaId::Requisition).kelvin,
            "the quarters must be the warmest room in the Site"
        );
        assert!(
            area_light(AreaId::Monitoring).kelvin < area_light(AreaId::Containment).kelvin,
            "monitoring must stay cooler than the wing it watches, or the boundary stops reading"
        );
        assert!(
            area_light(AreaId::Kitchen).kelvin < area_light(AreaId::WarRoom).kelvin,
            "the galley must be warmer than the planning room"
        );
    }
}
