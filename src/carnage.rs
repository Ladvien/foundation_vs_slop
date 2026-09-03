//! # Carnage — the game's half of `bevy_carnage`
//!
//! The fracture itself — the triangle soup, the recursive plane cuts, the watertight caps, the
//! bake-once-per-source cache — lives in `crates/bevy_carnage`, which knows nothing about this game.
//! What stays here is the part that is *this game's content*: VALKYRIE carries her rifle inside the
//! body scene, and the bake only runs while a run is active.
//!
//! **The crate's determinism record moved with the code.** `seed_from_path`'s writeup (why a fracture
//! seed may never come from an `AssetId`), the canonical vertex-soup sort, and the streaming gate that
//! treats an empty detached part as "not yet" rather than "absent" are all in
//! `crates/bevy_carnage/src/bake.rs`, with `crates/bevy_carnage/CLAUDE.md` carrying the summary. That
//! is FVS-N-8 and G0d; read them there before touching the bake.
//!
//! Spawning is `gore::spawn_fragments` — avian bodies, box colliders, the `GibRing` slot, the
//! `Carryable` weight the crabs forage on. The crate never learns any of it.

use bevy::prelude::*;
use bevy::camera::primitives::MeshAabb;

use crate::squad::{FigurineModel, FigurineSource, GunModel, Unit};

/// The baked fragment set, under the name every call site in this game already uses.
///
/// `CarnageCache` is `bevy_carnage::FractureCache`, and `GunChunk` is its `DetachedChunk` — the crate
/// must not say "gun", because a fracture library is useless to a project that has none.
pub use bevy_carnage::{DetachedChunk as GunChunk, Fragment, FractureCache as CarnageCache};

/// Registers the fracture cache and its one-shot bake, and gives them this game's schedule.
///
/// **Not a re-export of the crate's plugin**, because two things have to be added on top: the
/// authored [`FractureSettings`](bevy_carnage::FractureSettings) from `config.ron`, and the run gate.
/// `lib.rs` and `sim_harness.rs` both add `carnage::CarnagePlugin` and neither needed to change.
pub struct CarnagePlugin;

impl Plugin for CarnagePlugin {
    fn build(&self, app: &mut App) {
        // Required config — one path, no fallback. The `fracture:` slice comes from the unified
        // `assets/config/config.ron`, loaded + validated once by `ConfigPlugin` (registered first),
        // exactly as `GorePlugin` reads its own slice.
        //
        // Inserted BEFORE the crate's plugin on purpose: `CarnagePlugin` there `init_resource`s
        // `FractureSettings`, which does nothing when the resource already exists. So the authored
        // values win and the crate's `Default` only ever covers a standalone user. One owner, one
        // value — never a merge.
        let settings = app.world().resource::<crate::config::GameConfig>().fracture.clone();
        app.insert_resource(settings)
            .add_plugins(bevy_carnage::CarnagePlugin)
            // The crate deliberately configures no run condition — the caller owns the schedule.
            .configure_sets(
                Update,
                bevy_carnage::CarnageSystems.run_if(in_state(crate::session::RunState::Active)),
            )
            // Both run before the bake reads the scene: the rifle tag so the gun chunk is pruned out
            // of the body soup and the bake gate sees a non-empty detached part (see
            // `tag_valkyrie_rifle`), and the proxy so `bake_fractures`' query matches at all (see
            // `supply_humanoid_proxy`). The edge into the bake was a `.chain()` before the extraction;
            // the crate now exposes a set, so it can be stated directly instead of relying on tuple
            // order.
            //
            // **Chained to each other, not merely grouped.** `supply_humanoid_proxy` prunes the
            // `GunModel` subtree out of the box it measures, and `tag_valkyrie_rifle` is what puts
            // that marker on VALKYRIE's in-scene rifle. Without the sync point `.chain()` inserts,
            // the proxy would be built once — permanently, it is inserted only when absent — from a
            // box stretched by whatever she is holding.
            .add_systems(
                Update,
                (tag_valkyrie_rifle, supply_humanoid_proxy)
                    .chain()
                    .before(bevy_carnage::CarnageSystems)
                    .run_if(in_state(crate::session::RunState::Active)),
            );
    }
}

/// Marks a `FigurineModel` child whose in-scene rifle has already been tagged `GunModel`, so
/// [`tag_valkyrie_rifle`] runs once per unit. Lives on the cosmetic figurine child (never the `Unit`), so
/// it can't split the hashed squad archetype — same discipline as `squad::Recolored`.
#[derive(Component)]
struct RifleTagged;

/// VALKYRIE carries her rifle *inside* the body scene (a rigid mesh on the `spine_03` bone), not as the
/// separate held-item child the old greybox used. Once the scene streams in, find that `rifle` sub-mesh
/// and tag it `GunModel` so the bake prunes it into the intact, self-materialed gun chunk exactly as it
/// did the old blaster — the rifle still flies off on death, and the crate's "empty detached part means
/// still streaming" gate stays satisfied. Runs `.before(CarnageSystems)` so the tag is in place the same
/// frame the scene's meshes finish loading.
///
/// **The `"rifle"` node name is a content contract, which is exactly why this system did not move into
/// the crate.** It is authored in `characters/valkyrie.glb` and documented in `docs/artist_guide.md`;
/// a fracture library has no business knowing it.
fn tag_valkyrie_rifle(
    mut commands: Commands,
    figurines: Query<Entity, (With<FigurineModel>, Without<RifleTagged>)>,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    for figurine in &figurines {
        let mut stack: Vec<Entity> = match children.get(figurine) {
            Ok(c) => c.iter().collect(),
            Err(_) => continue, // scene not instantiated yet — retry next frame
        };
        let mut tagged = false;
        while let Some(e) = stack.pop() {
            if names.get(e).map(|n| n.as_str().contains("rifle")).unwrap_or(false) {
                // Tag the whole rifle node subtree as the gun chunk; don't descend past it.
                commands.entity(e).insert(GunModel);
                tagged = true;
                continue;
            }
            if let Ok(ch) = children.get(e) {
                stack.extend(ch.iter());
            }
        }
        if tagged {
            commands.entity(figurine).insert(RifleTagged);
        }
    }
}

/// The blockout every humanoid figurine is fractured against: `(centre, half-extents)` in
/// subject-local metres, one convex cell per limb.
///
/// **Six cells, never unioned, is what keeps the head separable from the torso.** One box would make
/// the whole body a single connected shell, so a cleaving blow could never take an arm off — the cell
/// decomposition *is* the set of places the body can come apart.
///
/// Copied from `crates/bevy_carnage/examples/common/body.rs::parts`, which is an *example* and so is
/// not reachable from this crate. These are the exact cells the crate's demos and its pinned
/// benchmark fracture.
const HUMANOID_PROXY: [(Vec3, Vec3); 6] = [
    (Vec3::new(0.00, 0.00, 0.0), Vec3::new(0.22, 0.32, 0.14)), // torso
    (Vec3::new(0.00, 0.46, 0.0), Vec3::new(0.13, 0.14, 0.13)), // head
    (Vec3::new(-0.32, 0.06, 0.0), Vec3::new(0.10, 0.26, 0.10)), // arm.L
    (Vec3::new(0.32, 0.06, 0.0), Vec3::new(0.10, 0.26, 0.10)), // arm.R
    (Vec3::new(-0.13, -0.62, 0.0), Vec3::new(0.11, 0.30, 0.12)), // leg.L
    (Vec3::new(0.13, -0.62, 0.0), Vec3::new(0.11, 0.30, 0.12)), // leg.R
];

/// Half the blockout's own height, `(0.60 − (−0.92)) / 2`. A figurine's measured half-height divided
/// by this is the single uniform factor every cell is scaled by.
const HUMANOID_HALF_HEIGHT: f32 = 0.76;

/// The blockout's own lowest point — the soles, `leg.y − leg.half_y = −0.62 − 0.30`.
///
/// **The blockout is anchored by its feet, not by its centre, and that is measured rather than
/// assumed.** `HUMANOID_PROXY` is centred on the torso (it spans y ∈ [−0.92, 0.60]); the game's
/// figurine is authored feet-at-origin — probed once, `y ∈ [−0.039, 1.615]`. Scaling alone would put
/// the scaled blockout at y ∈ [−1.00, 0.65]: both legs entirely below the mesh, the head cell at the
/// crotch, and a fracture of a shape the subject is not. Aligning the two *centres* instead would be
/// wrong for a different reason — the measured box is stretched sideways by whatever the figurine is
/// holding, so its centre is not the spine. Soles-to-lowest-point is the one landmark both
/// conventions agree on, and for a torso-origin subject it reduces to the identity.
const HUMANOID_FLOOR: f32 = -0.92;

/// Supply every `Unit` with the [`FractureProxy`](bevy_carnage::FractureProxy) the bake refuses to
/// invent, scaled to the figurine it actually has.
///
/// **Without this the bake never runs at all.** `bake_fractures` queries
/// `(&FractureSubject, &FractureProxy, …)`; `FigurineSource` *is* `FractureSubject`, but nothing in
/// this game ever built a `ProxyCell`, so the query matched nothing, the cache stayed empty and
/// `gore::spawn_fragments` took its `warn!` early-return on every death. The crate refuses to
/// synthesise a bounding box (`crates/bevy_carnage/CLAUDE.md`), and that refusal is right — a
/// synthesised box fractures a shape the subject is not, silently.
///
/// Runs `.before(CarnageSystems)` for the same reason [`tag_valkyrie_rifle`] does: the proxy has to
/// be in place the frame the scene's meshes finish loading, or the bake skips a frame per unit.
///
/// **A unit whose descendants carry no loaded `Mesh3d` yet is skipped and given nothing.** That is
/// the same streaming tolerance `bake_fractures` already applies; inserting a placeholder would bake
/// wrong geometry into a cache keyed per asset, and the mistake would be permanent.
fn supply_humanoid_proxy(
    mut commands: Commands,
    subjects: Query<
        Entity,
        (With<Unit>, With<FigurineSource>, Without<bevy_carnage::FractureProxy>),
    >,
    children: Query<&Children>,
    transforms: Query<&Transform>,
    mesh_q: Query<&Mesh3d>,
    is_detached: Query<(), With<GunModel>>,
    meshes: Res<Assets<Mesh>>,
) {
    for subject in &subjects {
        let Ok(top) = children.get(subject) else { continue }; // scene not instantiated yet
        // DFS of (entity, transform relative to the subject root), pruning the detached part exactly
        // as `bake_fractures` does — the rifle is baked as its own chunk and is not body geometry, so
        // it must not stretch the measurement the limb cells are scaled by.
        let mut stack: Vec<(Entity, Mat4)> = Vec::new();
        for child in top.iter() {
            if is_detached.get(child).is_ok() {
                continue;
            }
            let m = transforms.get(child).map(|t| t.to_matrix()).unwrap_or(Mat4::IDENTITY);
            stack.push((child, m));
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        while let Some((e, mat)) = stack.pop() {
            if let Ok(mesh3d) = mesh_q.get(e)
                && let Some(mesh) = meshes.get(&mesh3d.0)
                && let Some(aabb) = mesh.compute_aabb()
            {
                let c: Vec3 = aabb.center.into();
                let h: Vec3 = aabb.half_extents.into();
                for corner in 0..8u32 {
                    let signs = Vec3::new(
                        if corner & 1 == 0 { -1.0 } else { 1.0 },
                        if corner & 2 == 0 { -1.0 } else { 1.0 },
                        if corner & 4 == 0 { -1.0 } else { 1.0 },
                    );
                    let p = mat.transform_point3(c + signs * h);
                    min = min.min(p);
                    max = max.max(p);
                }
            }
            if let Ok(ch) = children.get(e) {
                for child in ch.iter() {
                    if is_detached.get(child).is_ok() {
                        continue;
                    }
                    let ct = transforms.get(child).map(|t| t.to_matrix()).unwrap_or(Mat4::IDENTITY);
                    stack.push((child, mat * ct));
                }
            }
        }
        if !min.is_finite() || !max.is_finite() {
            continue; // no loaded body mesh yet — retry next frame
        }
        let half = (max - min) * 0.5;
        // One uniform factor from the measured half-height. Uniform, not per-axis: a non-uniform
        // scale would shear the limb cells away from the mesh they are meant to approximate.
        let scale = (half.y / HUMANOID_HALF_HEIGHT).clamp(0.25, 4.0);
        // Then slide the scaled blockout up so its soles sit on the figurine's lowest point. X and Z
        // stay on the subject's own axis, which for an upright character is the spine.
        let lift = Vec3::Y * (min.y - HUMANOID_FLOOR * scale);
        let cells = HUMANOID_PROXY
            .iter()
            .map(|(c, h)| bevy_carnage::ProxyCell::from_box(*c * scale + lift, *h * scale))
            .collect();
        commands.entity(subject).insert(bevy_carnage::FractureProxy(cells));
    }
}
