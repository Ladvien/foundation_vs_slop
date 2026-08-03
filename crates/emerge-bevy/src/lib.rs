//! **emerge-bevy** — turning a library and a map into entities.
//!
//! `emerge-core` is the schema, the solvers and the validation, and it has no engine in it;
//! `crates/emerge-core/tests/engine_free.rs` fails the build if that stops being true. This is the
//! other side: one function that puts a [`Descriptor`] in a world, and a plugin that puts a whole
//! [`Map`] there.
//!
//! # One spawner, two callers
//!
//! `emerge-mapper` shows you a map and the game plays it, and they must agree about what a placement
//! looks like down to the last degree of yaw. The way to guarantee that is not care — it is having
//! one function. `sim_harness.rs` is the precedent this project already trusts: the same plugin graph
//! from two entry points, *"not a second code path"*.
//!
//! The alternative was tried elsewhere in this tree and is on the record: `bake.rs` and
//! `site_editor::source_map` independently grew the same RON writer, and one of them had a bug the
//! other did not.
//!
//! # What a placement carries
//!
//! More than a mesh. `docs/2026-08-03-asset-schema-audit.md` §2 is blunt about the state this
//! replaces: *"A placed prop keeps only `PlacedIn(RegionId)`"* — a bare newtype — so every downstream
//! system treats furniture as anonymous geometry, and of eight affordance tokens exactly one has any
//! runtime consequence. A piece spawned here keeps its id, its resolved [`Tags`], and its
//! [`MountedOn`] host, so "what is on this worktop?" and "what here recharges stamina?" are questions
//! the world can answer.
//!
//! # `MountedOn` is a relationship
//!
//! Not a bare component. This repo already runs three relationship pairs — `HeldAt`/`SiteSpecimens`,
//! `MemberOf`/`SquadRoster`, `Holding`/`HeldBy` — so the reverse index comes from the engine rather
//! than from a hand-maintained map. Both gotchas those pairs document apply here and are restated on
//! the types.

use bevy::prelude::*;
use emerge_core::descriptor::{Descriptor, Mount};
use emerge_core::library::Library;
use emerge_core::map::Map;
use emerge_core::vocab::{Masks, Vocabularies};

/// The library, map and vocabulary a world was built from.
#[derive(Resource)]
pub struct EmergeWorld {
    pub library: Library,
    pub map: Map,
    pub vocab: Vocabularies,
    /// Per-descriptor masks, in library order — resolved once at load.
    pub masks: Vec<Masks>,
}

impl EmergeWorld {
    /// Validate a library and map together and keep the resolved masks.
    ///
    /// Refuses rather than loading half of it: a map that names a descriptor nothing defines is a map
    /// with holes, and the piece silently failing to appear is how nobody finds out.
    pub fn new(library: Library, map: Map, vocab: Vocabularies) -> Result<EmergeWorld, String> {
        let masks = library.resolve(&vocab)?;
        map.validate()?;
        let known: Vec<&str> = library.descriptors.iter().map(|d| d.id.as_str()).collect();
        let missing: Vec<&str> = map
            .placements
            .iter()
            .filter(|p| !known.contains(&p.descriptor.as_str()))
            .map(|p| p.descriptor.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "map `{}` places {} descriptor(s) this library does not define: {missing:?}",
                map.name,
                missing.len()
            ));
        }
        Ok(EmergeWorld {
            library,
            map,
            vocab,
            masks,
        })
    }

    /// A descriptor and its masks by id.
    pub fn entry(&self, id: &str) -> Option<(&Descriptor, Masks)> {
        let ix = self.library.descriptors.iter().position(|d| d.id == id)?;
        Some((&self.library.descriptors[ix], *self.masks.get(ix)?))
    }
}

/// The placement's stable id from the map — how an entity is named back to the file it came from.
#[derive(Component, Debug, Clone)]
pub struct Placement(pub String);

/// The descriptor this entity is an instance of.
#[derive(Component, Debug, Clone)]
pub struct OfDescriptor(pub String);

/// The resolved token masks, so a query for "what here emits light?" is one `&` rather than a string
/// comparison against a `Vec<String>`.
///
/// Game AI Pro 4 ch.4 on smart-object matching: *"a simple bit-mask can be used to represent the
/// requirements for the link and the capabilities of the agent. Comparing these bitmasks is a very
/// efficient way to filter out invalid links."*
#[derive(Component, Debug, Clone, Copy)]
pub struct Tags(pub Masks);

/// "This piece rests on that one."
///
/// A relationship rather than a bare component, so the reverse index is the engine's job. Two gotchas
/// this repo's other three pairs document, both of which apply:
///
/// 1. **An empty target is expressed by REMOVING the component**, so a reader must always take
///    `Option<&Supporting>` and never assume the component is present on a host with nothing on it.
/// 2. **Target order is attach order, never a total order.** Anything that picks one — a lethal
///    choice, a shared RNG draw, a `take(n)` — must sort by a stable key first, or it depends on spawn
///    order and stops being reproducible. See `docs/…` on `sort_total!`.
#[derive(Component, Debug)]
#[relationship(relationship_target = Supporting)]
pub struct MountedOn(pub Entity);

/// "These pieces rest on me." The reverse of [`MountedOn`], maintained by the engine.
#[derive(Component, Debug)]
#[relationship_target(relationship = MountedOn)]
pub struct Supporting(Vec<Entity>);

/// Spawns an [`EmergeWorld`] when one is inserted.
pub struct EmergePlugin;

impl Plugin for EmergePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            spawn_world.run_if(resource_added::<EmergeWorld>),
        );
    }
}

/// Where a piece's origin goes for a map position — **the** arithmetic, used by the editor's preview
/// and by the game, so a map cannot look different in the two.
pub fn origin_of(d: &Descriptor, at: (f32, f32), floor_y: f32) -> Vec3 {
    let lift = match &d.mount {
        Some(Mount::OnWall { height }) => *height,
        Some(Mount::OnCeiling) => 2.4,
        _ => 0.0,
    };
    Vec3::new(
        at.0,
        floor_y + lift + d.align.y_offset.unwrap_or(0.0),
        at.1,
    )
}

/// The yaw a piece is actually drawn at: the authored yaw plus the mesh's own `front` correction.
///
/// The correction is not cosmetic. The Site kit records a 90° `front` on its chairs, and before that
/// existed every chair in `site67.ron` was authored sideways to its table — the yaws were written
/// against the engine convention while the mesh fronted somewhere else.
pub fn draw_yaw(d: &Descriptor, authored: f32) -> f32 {
    authored + d.align.front.unwrap_or(0.0)
}

/// Put one descriptor in the world.
///
/// Returns `None` for a descriptor with no mesh — which is not an error: a descriptor may exist to
/// carry tags before anyone has given it geometry.
pub fn spawn_descriptor(
    commands: &mut Commands,
    assets: &AssetServer,
    d: &Descriptor,
    masks: Masks,
    at: (f32, f32),
    yaw: f32,
    floor_y: f32,
) -> Option<Entity> {
    let mesh = d.mesh.as_ref()?;
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(mesh.clone()));
    let scale = d.align.scale.unwrap_or(1.0);
    let stretch = d.align.stretch_y.unwrap_or(1.0);
    Some(
        commands
            .spawn((
                Name::new(d.id.clone()),
                OfDescriptor(d.id.clone()),
                Tags(masks),
                Transform::from_translation(origin_of(d, at, floor_y))
                    .with_rotation(Quat::from_rotation_y(draw_yaw(d, yaw).to_radians()))
                    // Y is scaled separately: `stretch_y` is a project's architecture policy layered
                    // over the mesh's measured height, not an art correction.
                    .with_scale(Vec3::new(scale, scale * stretch, scale)),
                Visibility::Inherited,
            ))
            .with_child((WorldAssetRoot(scene), Transform::default()))
            .id(),
    )
}

/// Spawn every placement in the map.
fn spawn_world(mut commands: Commands, assets: Res<AssetServer>, world: Res<EmergeWorld>) {
    let floor_y = world.map.origin.1;
    let mut spawned = 0usize;
    for p in &world.map.placements {
        let Some((d, masks)) = world.entry(&p.descriptor) else {
            // `EmergeWorld::new` refuses this, so reaching it means the resource was built by hand.
            warn!("`{}` names undefined descriptor `{}`", p.id, p.descriptor);
            continue;
        };
        if let Some(e) = spawn_descriptor(&mut commands, &assets, d, masks, p.at, p.yaw, floor_y) {
            commands.entity(e).insert(Placement(p.id.clone()));
            spawned += 1;
        }
    }
    info!(
        "emerge: spawned {spawned} of {} placement(s) from map `{}`",
        world.map.placements.len(),
        world.map.name
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use emerge_core::descriptor::{Align, Extent};
    use emerge_core::library::LIBRARY_VERSION;
    use emerge_core::map::Placed;

    fn descriptor(id: &str) -> Descriptor {
        Descriptor {
            id: id.to_owned(),
            mesh: Some(format!("{id}.glb")),
            extent: Extent {
                footprint: Some((1.0, 1.0)),
                height: Some(1.0),
            },
            ..Descriptor::default()
        }
    }

    fn world(descriptors: Vec<Descriptor>, placements: Vec<Placed>) -> Result<EmergeWorld, String> {
        EmergeWorld::new(
            Library {
                version: LIBRARY_VERSION,
                note: None,
                descriptors,
            },
            Map {
                name: "test_map".into(),
                placements,
                ..Map::default()
            },
            Vocabularies::default(),
        )
    }

    fn placed(id: &str, descriptor: &str) -> Placed {
        Placed {
            id: id.to_owned(),
            descriptor: descriptor.to_owned(),
            at: (0.0, 0.0),
            ..Placed::default()
        }
    }

    /// **A map with a hole in it is refused at load.** The piece silently failing to appear is how
    /// nobody finds out — a room one chair short looks exactly like a room the layout put one chair in.
    #[test]
    fn a_map_naming_an_undefined_descriptor_is_refused() {
        let err = world(vec![descriptor("crate")], vec![placed("a", "ghost")])
            .err()
            .unwrap_or_default();
        assert!(err.contains("does not define"), "{err}");
        assert!(err.contains("ghost"), "must name it: {err}");
    }

    #[test]
    fn a_consistent_library_and_map_load() {
        let w = world(vec![descriptor("crate")], vec![placed("a", "crate")])
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(w.entry("crate").is_some());
        assert!(w.entry("nothing").is_none());
    }

    /// The mesh's own facing correction is applied on top of the authored yaw. Without it every chair
    /// in `site67.ron` was authored sideways to its table.
    #[test]
    fn the_draw_yaw_adds_the_meshs_front_correction() {
        let mut d = descriptor("chair");
        d.align.front = Some(90.0);
        assert_eq!(draw_yaw(&d, 0.0), 90.0);
        assert_eq!(draw_yaw(&d, 45.0), 135.0);

        // No recorded front means no correction — `None` is a claim that the mesh is symmetric, and
        // defaulting it to zero would be indistinguishable from `Some(0.0)`.
        let plain = descriptor("stool");
        assert_eq!(draw_yaw(&plain, 30.0), 30.0);
    }

    /// The layer decides the height, and the map's own floor is the datum — so a map placed at y=10
    /// hangs its sconces at 11.8 rather than at 1.8.
    #[test]
    fn the_mount_decides_the_height_above_the_maps_floor() {
        let mut floor = descriptor("crate");
        floor.mount = Some(Mount::OnFloor);
        assert_eq!(origin_of(&floor, (2.0, 3.0), 0.0), Vec3::new(2.0, 0.0, 3.0));
        assert_eq!(origin_of(&floor, (2.0, 3.0), 10.0), Vec3::new(2.0, 10.0, 3.0));

        let mut sconce = descriptor("sconce");
        sconce.mount = Some(Mount::OnWall { height: 1.8 });
        assert_eq!(origin_of(&sconce, (0.0, 0.0), 10.0).y, 11.8);
    }

    /// `y_offset` is a geometric correction and stacks with the mount's lift rather than replacing it.
    #[test]
    fn the_alignment_offset_stacks_with_the_mount() {
        let mut grate = descriptor("floor_grate");
        grate.mount = Some(Mount::OnFloor);
        grate.align = Align {
            y_offset: Some(-0.06),
            ..Align::default()
        };
        assert!((origin_of(&grate, (0.0, 0.0), 0.0).y + 0.06).abs() < 1e-6);
    }
}
