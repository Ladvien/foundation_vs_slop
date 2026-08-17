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

use std::collections::HashMap;

use bevy::asset::AssetId;
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::prelude::*;
use emerge_core::descriptor::Descriptor;
use emerge_core::library::Library;
use emerge_core::map::Map;
use emerge_core::smart::{self, Actor, Booking, Cast, RoleMasks, Seat, Unfilled};
use emerge_core::vocab::{Masks, Vocabularies};

/// The library, map and vocabulary a world was built from.
#[derive(Resource)]
pub struct EmergeWorld {
    pub library: Library,
    pub map: Map,
    pub vocab: Vocabularies,
    /// Per-descriptor masks, in library order — resolved once at load.
    pub masks: Vec<Masks>,
    /// World Y for every placement, in map order — resolved once at load by
    /// [`emerge_core::stack::resolve_y`], because a piece's height can depend on a piece authored
    /// later in the file.
    pub y: Vec<f32>,
    /// Every role's capability requirement, resolved once at load.
    pub roles: RoleMasks,
    /// Every seat every location offers, in world space, keyed by location id and in a total order.
    ///
    /// Computed at load rather than per query. A socket's world position moves only when the map does,
    /// and the map does not move at runtime — so recomputing it inside the loop that runs whenever an
    /// agent looks for something to do would be work done thousands of times to get the same answer.
    pub seats: Vec<(String, Vec<Seat>)>,
}

impl EmergeWorld {
    /// Validate a library and map together and keep the resolved masks.
    ///
    /// Refuses rather than loading half of it: a map that names a descriptor nothing defines is a map
    /// with holes, and the piece silently failing to appear is how nobody finds out.
    pub fn new(library: Library, map: Map, vocab: Vocabularies) -> Result<EmergeWorld, String> {
        Self::with_compositions(library, map, vocab, &[])
    }

    /// The same, for a map that stamps compositions.
    ///
    /// **Expansion happens once, here, before anything else looks at the map.** Everything downstream
    /// — masks, `resolve_y`, roles, seats, the spawner — then sees a flat world exactly as it did
    /// before stamps existed, which is what keeps this a schema addition rather than a second way for
    /// a map to mean something.
    ///
    /// The expanded rows are folded into [`Self::map`] rather than kept beside it, because every
    /// reader downstream indexes `map.placements` in step with [`Self::y`]. What is *not* done is
    /// writing them back to disk: `Map::stamps` is what the file holds, and
    /// `emerge_core::composition::expand` is the only thing that turns it into rows.
    pub fn with_compositions(
        library: Library,
        mut map: Map,
        vocab: Vocabularies,
        compositions: &[emerge_core::composition::Composition],
    ) -> Result<EmergeWorld, String> {
        let masks = library.resolve(&vocab)?;
        // **Expand first, then validate once.** Validating before expansion checks a map that is not
        // the map — `Map::validate` resolves `locations[].props` against `placements`, so a
        // map-level location over a stamped row ("clean up `mess_a/table`") was refused for naming a
        // placement that does not exist *yet*. Expansion is what makes the map whole; validation is
        // the question asked of a whole map, and asking it twice about two different maps is two
        // answers where the schema promises one.
        if !map.stamps.is_empty() {
            let expanded = emerge_core::composition::expand(&map, &map.stamps, compositions, &library)
                .map_err(|e| format!("map `{}`: {e}", map.name))?;
            map.placements.extend(expanded.placements);
            map.locations.extend(expanded.locations);
        }
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
        // **Every height, now.** A map whose stacking cannot be resolved is refused here rather than
        // discovered as a lamp inside a table — the same argument the missing-descriptor check above
        // makes, applied to the axis the schema could describe and nothing implemented.
        let y = emerge_core::stack::resolve_y(&map, &library)?;
        // Roles and seats resolved with everything else, so a misspelled capability or a location
        // governing a prop nobody placed is refused here rather than surfacing as a scene that
        // silently never starts — the least debuggable shape a content bug takes.
        let roles = smart::resolve_roles(&map, &vocab)?;
        let mut seats = Vec::with_capacity(map.locations.len());
        for loc in &map.locations {
            seats.push((loc.id.clone(), smart::seats_of(&map, &library, &y, loc)?));
        }
        Ok(EmergeWorld {
            library,
            map,
            vocab,
            masks,
            y,
            roles,
            seats,
        })
    }

    /// The seats a location offers.
    pub fn seats(&self, location: &str) -> &[Seat] {
        self.seats
            .iter()
            .find(|(id, _)| id == location)
            .map_or(&[], |(_, s)| s.as_slice())
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
/// **Paint order, carried onto a spawned piece** — see [`emerge_core::map::Placed::paint`].
///
/// Put on the root a piece spawns as; [`apply_paint`] finds it by walking up from each material the
/// glTF scene brings in, because those arrive frames later than the entity does.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paint(pub i8);

/// **One material per `(base, paint)` pair, never per piece.**
///
/// A clone per instance would make material count scale with how many things are in the level, which
/// defeats batching — and `docs/bevy_plugins.md` is explicit that a kit is instance-heavy and must
/// batch by mesh and material. Keyed this way it scales with kinds x layers instead, and `paint`
/// being an `i8` is what bounds the second factor.
#[derive(Resource, Default)]
pub struct PaintedMaterials(HashMap<(AssetId<StandardMaterial>, i8), Handle<StandardMaterial>>);

/// How much depth bias one paint step is worth.
///
/// Small on purpose. `depth_bias` biases the **depth comparison**, so this reorders surfaces that are
/// already close — two decals on one floor, which is what the field is for. It is deliberately not
/// enough to hoist something in front of unrelated geometry it sits well behind; that would be a
/// different feature and a worse one, because it would let a tile's dressing punch through its walls.
const PAINT_STEP: f32 = 1.0;

/// **Give every material under a painted piece its biased twin.**
///
/// Runs on materials as they *arrive*: `spawn_descriptor` hands Bevy a glTF scene handle, so the
/// meshes and their materials are inserted some frames after the entity exists. There is no
/// spawn-time hook to do this in, which is why it is a system watching `Added` rather than a line in
/// the spawner.
fn apply_paint(
    mut commands: Commands,
    mut cache: ResMut<PaintedMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    arrived: Query<(Entity, &MeshMaterial3d<StandardMaterial>), Added<MeshMaterial3d<StandardMaterial>>>,
    parents: Query<&ChildOf>,
    painted: Query<&Paint>,
) {
    for (entity, material) in &arrived {
        // Walk up to the piece root. A glTF scene nests arbitrarily deep, so the paint is never on
        // the entity holding the material.
        let Some(paint) = std::iter::successors(Some(entity), |e| parents.get(*e).ok().map(|p| p.0))
            .find_map(|e| painted.get(e).ok())
            .map(|p| p.0)
            .filter(|p| *p != 0)
        else {
            continue;
        };
        let base = material.0.id();
        let biased = match cache.0.get(&(base, paint)) {
            Some(h) => h.clone(),
            None => {
                let Some(source) = materials.get(base) else { continue };
                let mut copy = source.clone();
                copy.depth_bias += paint as f32 * PAINT_STEP;
                let handle = materials.add(copy);
                cache.0.insert((base, paint), handle.clone());
                handle
            }
        };
        commands.entity(entity).insert(MeshMaterial3d(biased));
    }
}

pub struct EmergePlugin;

impl Plugin for EmergePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SmartObjects>()
            .init_resource::<PaintedMaterials>()
            .add_systems(
                Update,
                spawn_world.run_if(resource_added::<EmergeWorld>),
            )
            // Not gated on the world resource: a scene's materials land frames after the spawn, and
            // the editor spawns pieces without an `EmergeWorld` at all.
            .add_systems(Update, apply_paint);
    }
}

/// Where a piece's origin goes in the world — **the** arithmetic, used by the editor's preview and by
/// the game, so a map cannot look different in the two.
///
/// # Map space is not world space
///
/// `Placed::at` is *"position in map space"* and `Map::origin` is *"where this map sits in the world:
/// the centre of its floor"*. The first version added only the origin's Y and used `at` directly for
/// X and Z, which is correct for exactly one map — the one at the origin — and silently wrong for
/// every other. The editor authors at `(0, 0, 0)` so nothing showed it.
///
/// A map is a thing you place, not a thing that is already placed, and the whole point of the field
/// is dropping the same authored room into two different corners of a level.
///
/// # Y comes from the stack, not from here
///
/// The height a piece sits at depends on the map's ceiling and on whatever it is resting on, neither
/// of which is knowable from the descriptor alone. [`emerge_core::stack::resolve_y`] answers it for a
/// whole map at once; this takes the answer. The version that tried to decide it here matched two
/// mount variants and sent the rest to `_ => 0.0`, which put every `OnSurface` piece in the library on
/// the floor.
pub fn origin_of(at: (f32, f32), map_origin: (f32, f32, f32), y: f32) -> Vec3 {
    Vec3::new(map_origin.0 + at.0, y, map_origin.2 + at.1)
}

/// The yaw a piece is actually drawn at: the authored yaw plus the mesh's own `front` correction.
///
/// The correction is not cosmetic. The Site kit records a 90° `front` on its chairs, and before that
/// existed every chair in `site67.ron` was authored sideways to its table — the yaws were written
/// against the engine convention while the mesh fronted somewhere else.
pub fn draw_yaw(d: &Descriptor, authored: f32) -> f32 {
    authored + d.align.front.map_or(0.0, |f| f.yaw_degrees())
}

/// The world rotation of a placement's tip: about X first, then Z — the same order
/// [`mesh_rotation`] documents, so "which axis first" has one answer in the whole codebase.
pub fn tip_quat(tip: (u8, u8)) -> Quat {
    Quat::from_rotation_z((tip.1 as f32 * 90.0).to_radians())
        * Quat::from_rotation_x((tip.0 as f32 * 90.0).to_radians())
}

/// How far a tipped piece must rise so its bounds rest **on** the resolved height instead of
/// through it.
///
/// A mesh is authored foot-at-origin (the importer's contract), so its placed box is
/// `x ∈ ±w/2, y ∈ 0..h, z ∈ ±depth/2` — and a quarter turn about that origin swings half the box
/// below the floor. The seat is the exact counter-lift: rotate the eight corners, take the lowest,
/// come back up by that much. Quarter turns make it exact, not approximate.
///
/// Zero for an untipped piece, and zero for an unmeasured one — the editor refuses to tip what it
/// cannot seat, which is the loud half of this bargain.
pub fn tip_seat(d: &Descriptor, tip: (u8, u8)) -> f32 {
    if tip == (0, 0) {
        return 0.0;
    }
    let (Some((w, depth)), Some(h)) = (
        emerge_core::descriptor::placed_footprint(d),
        emerge_core::descriptor::placed_height(d),
    ) else {
        return 0.0;
    };
    let q = tip_quat(tip);
    let mut min_y = f32::MAX;
    for sx in [-0.5_f32, 0.5] {
        for sy in [0.0_f32, 1.0] {
            for sz in [-0.5_f32, 0.5] {
                let corner = Vec3::new(sx * w, sy * h, sz * depth);
                min_y = min_y.min((q * corner).y);
            }
        }
    }
    -min_y
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
    tip: (u8, u8),
    map_origin: (f32, f32, f32),
    y: f32,
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
                // The tip turns the piece in its own frame, the yaw turns the tipped piece, and the
                // seat keeps the result standing on `y` — see [`tip_seat`].
                Transform::from_translation(origin_of(at, map_origin, y + tip_seat(d, tip)))
                    .with_rotation(
                        Quat::from_rotation_y(draw_yaw(d, yaw).to_radians()) * tip_quat(tip),
                    )
                    // Y is scaled separately: `stretch_y` is a project's architecture policy layered
                    // over the mesh's measured height, not an art correction.
                    .with_scale(Vec3::new(scale, scale * stretch, scale)),
                Visibility::Inherited,
            ))
            // **The default rotation rides the mesh child, not the placement.**
            //
            // The parent carries what the *world* says — where the piece stands, which way it was
            // turned, how this project stretches it. `align.rotate` says something else: which way
            // the artist happened to export the file. Putting it here keeps those separable, and it
            // makes the composition right for free — the parent's Y stretch is applied after this
            // rotation, so it stretches the piece's real height rather than whichever mesh axis
            // happens to point up in the file.
            .with_child((
                WorldAssetRoot(scene),
                Transform::from_rotation(mesh_rotation(d)),
            ))
            .id(),
    )
}

/// The mesh's own default rotation as a quaternion, or the identity.
///
/// Composed `Rz * Ry * Rx` so a point is turned about X, then Y, then Z — the order
/// `Align::rotate` documents and `glb::Measured::rotated` bakes the extent with. Written out rather
/// than through `Quat::from_euler`, because the euler-sequence conventions are exactly the kind of
/// thing that is off by an axis for a week.
///
/// A rotation that is not a quarter turn cannot be reached through the editor and is refused by
/// `quarter_turns_xyz` at load; if one arrives here anyway it is applied as written, because
/// silently squaring it would draw the mesh at an angle its extent does not describe.
pub fn mesh_rotation(d: &Descriptor) -> Quat {
    let Some((x, y, z)) = d.align.rotate else {
        return Quat::IDENTITY;
    };
    Quat::from_rotation_z((z as f32).to_radians())
        * Quat::from_rotation_y((y as f32).to_radians())
        * Quat::from_rotation_x((x as f32).to_radians())
}

/// Spawn every placement in the map.
///
/// Heights are resolved for the whole map first — a piece resting on a table needs the table's height,
/// and the table may be authored after it. `EmergeWorld::new` has already resolved them once and
/// refused the map if it could not, so this cannot fail here.
fn spawn_world(mut commands: Commands, assets: Res<AssetServer>, world: Res<EmergeWorld>) {
    let origin = world.map.origin;
    let mut spawned = 0usize;
    // Index by placement id so the `MountedOn` relationship can be attached once every entity exists.
    let mut by_id: Vec<(&str, Entity)> = Vec::with_capacity(world.map.placements.len());
    for (i, p) in world.map.placements.iter().enumerate() {
        let Some((d, masks)) = world.entry(&p.descriptor) else {
            // `EmergeWorld::new` refuses this, so reaching it means the resource was built by hand.
            warn!("`{}` names undefined descriptor `{}`", p.id, p.descriptor);
            continue;
        };
        let Some(&y) = world.y.get(i) else { continue };
        if let Some(e) =
            spawn_descriptor(&mut commands, &assets, d, masks, p.at, p.yaw, p.tip, origin, y)
        {
            commands.entity(e).insert(Placement(p.id.clone()));
            if p.paint != 0 {
                commands.entity(e).insert(Paint(p.paint));
            }
            by_id.push((p.id.as_str(), e));
            spawned += 1;
        }
    }

    // **`MountedOn` second**, because a host may be spawned after its guest and a relationship needs
    // an `Entity` that exists. The reverse index (`Supporting`) is the engine's to maintain.
    for p in &world.map.placements {
        let (Some(host_id), Some(&(_, guest))) = (
            p.on.as_ref(),
            by_id.iter().find(|(id, _)| *id == p.id.as_str()),
        ) else {
            continue;
        };
        match by_id.iter().find(|(id, _)| id == host_id) {
            Some(&(_, host)) => {
                commands.entity(guest).insert(MountedOn(host));
            }
            // Only reachable when the host is a descriptor with no mesh, so nothing was spawned for
            // it. The guest keeps its resolved height — it is standing on the right surface — but
            // nothing can answer "what is on this?", and that is worth saying.
            None => warn!(
                "`{}` rests on `{host_id}`, which spawned no entity — the height is right but the \
                 relationship is missing",
                p.id
            ),
        }
    }

    info!(
        "emerge: spawned {spawned} of {} placement(s) from map `{}`",
        world.map.placements.len(),
        world.map.name
    );
}

// ── smart objects ────────────────────────────────────────────────────────────────────────────────

/// What the world affords, and who is currently doing it.
///
/// The index and the bookings in one resource, because they answer one question between them: *what
/// could this agent do that nobody else is already doing?* Splitting them would let a query return a
/// table that a scene started on two systems ago.
#[derive(Resource, Default)]
pub struct SmartObjects {
    booking: Booking,
}

/// An interaction an agent could start: where, what, and how far away.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub location: String,
    pub verb: String,
    /// Metres from the asking agent to the location's nearest seat.
    pub distance: f32,
}

impl SmartObjects {
    /// **What is on offer near `from`.** Ordered nearest first, ties broken by `(location, verb)`.
    ///
    /// The query is the easy half — both chapters say so, and it is a filter and a sort. What it must
    /// not do is offer something unusable: a location already running a scene owns its props, so it is
    /// skipped rather than returned and refused later.
    ///
    /// The tie-break is a total order and not decoration. Two identical tables equidistant from an
    /// agent would otherwise be ranked by whatever order the map file happens to list them in, which
    /// is stable right up until somebody reorders the file.
    pub fn offers(&self, world: &EmergeWorld, from: Vec3, within: f32) -> Vec<Offer> {
        let mut out = Vec::new();
        for loc in &world.map.locations {
            if self.booking.is_busy(&loc.id) {
                continue;
            }
            let seats = world.seats(&loc.id);
            // A location with no seats is reachable from anywhere: its interactions want bystanders,
            // not marked spots, so distance is measured to the props' own group rather than to a chair.
            let distance = seats
                .iter()
                .map(|s| Vec3::new(s.at.0, s.at.1, s.at.2).distance(from))
                .fold(f32::INFINITY, f32::min);
            let distance = if distance.is_finite() { distance } else { 0.0 };
            if distance > within {
                continue;
            }
            for interaction in &loc.interactions {
                out.push(Offer {
                    location: loc.id.clone(),
                    verb: interaction.verb.clone(),
                    distance,
                });
            }
        }
        out.sort_by(|a, b| {
            a.distance
                .partial_cmp(&b.distance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| (&a.location, &a.verb).cmp(&(&b.location, &b.verb)))
        });
        out
    }

    /// Try to start `verb` at `location` with the actors offered.
    ///
    /// Only the free ones are put forward — *"each NPC only participates in at most one script at a
    /// time"* — so the exclusivity rule is enforced by what the allocator is allowed to see rather
    /// than by checking afterwards.
    pub fn start(
        &mut self,
        world: &EmergeWorld,
        location: &str,
        verb: &str,
        actors: &[Actor],
        rng: &mut impl emerge_core::rng::DetRng,
    ) -> Result<Cast, Unfilled> {
        let Some(loc) = world.map.locations.iter().find(|l| l.id == location) else {
            return Err(Unfilled::Role {
                role: format!("<no location `{location}`>"),
                need: 0,
                found: 0,
            });
        };
        let Some(interaction) = loc.interactions.iter().find(|i| i.verb == verb) else {
            return Err(Unfilled::Role {
                role: format!("<no `{verb}` at `{location}`>"),
                need: 0,
                found: 0,
            });
        };
        let requires = world.roles.get(location, verb).unwrap_or(&[]);
        let free = self.booking.free(actors);
        let cast = smart::allocate(
            loc,
            interaction,
            requires,
            &free,
            world.seats(location),
            rng,
        )?;
        // `allocate` saw only free actors and an idle location, so this cannot refuse — and if it ever
        // does, the invariant broke and silence would be the worst possible response.
        if let Err(e) = self.booking.start(cast.clone()) {
            error!("emerge: booking refused a cast it should have accepted: {e}");
        }
        Ok(cast)
    }

    /// End whatever `location` was running, freeing its props and its cast.
    pub fn finish(&mut self, location: &str) -> Option<Cast> {
        self.booking.finish(location)
    }

    pub fn is_busy(&self, location: &str) -> bool {
        self.booking.is_busy(location)
    }

    pub fn is_engaged(&self, actor: u64) -> bool {
        self.booking.is_engaged(actor)
    }

    pub fn running(&self) -> &[Cast] {
        self.booking.casts()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emerge_core::descriptor::Extent;
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
        d.align.front = Some(emerge_core::descriptor::Face::East);
        assert_eq!(draw_yaw(&d, 0.0), 90.0);
        assert_eq!(draw_yaw(&d, 45.0), 135.0);

        // No recorded front means no correction — `None` is a claim that the mesh is symmetric, and
        // defaulting it to zero would be indistinguishable from `Some(0.0)`.
        let plain = descriptor("stool");
        assert_eq!(draw_yaw(&plain, 30.0), 30.0);
    }

    // ── smart objects ────────────────────────────────────────────────────────────────────────

    /// A galley: one table with `seats` chairs, and one `eat` interaction wanting diners.
    fn galley(seats: usize) -> EmergeWorld {
        use emerge_core::descriptor::{Offers, Socket};
        use emerge_core::map::{Effect, Interaction, Location, RoleKind, RoleSlot};
        use emerge_core::vocab::{Vocabulary, Vocabularies};

        let mut table = descriptor("table");
        table.offers = Offers {
            surfaces: vec![],
            faces: vec![],
            sockets: (0..seats)
                .map(|i| Socket {
                    id: format!("seat_{i}"),
                    role: Some("diner".into()),
                    at: (i as f32, 0.45, 0.0),
                    yaw: 0.0,
                })
                .collect(),
        };

        let mut diner = RoleSlot {
            name: "diner".into(),
            kind: RoleKind::Main,
            min: 1,
            max: 4,
            socket_role: Some("diner".into()),
            requires: vec!["eat".into()],
        };
        diner.max = seats.max(1) as u8;

        EmergeWorld::new(
            Library {
                version: LIBRARY_VERSION,
                note: None,
                descriptors: vec![table],
            },
            Map {
                name: "test_map".into(),
                placements: vec![placed("t1", "table")],
                locations: vec![Location {
                    id: "galley_table_1".into(),
                    props: vec!["t1".into()],
                    interactions: vec![Interaction {
                        verb: "eat".into(),
                        roles: vec![diner],
                        guard: None,
                        effects: vec![Effect::Restore {
                            drive: "stamina".into(),
                            rate: 0.2,
                        }],
                        note: None,
                    }],
                    note: None,
                }],
                ..Map::default()
            },
            Vocabularies {
                capabilities: Vocabulary::of(&[("eat", "can take a meal")]),
                ..Vocabularies::default()
            },
        )
        .unwrap_or_else(|e| panic!("{e}"))
    }

    fn diners(n: u64) -> Vec<Actor> {
        use emerge_core::vocab::Can;
        (1..=n).map(|id| Actor { id, can: Can(1) }).collect()
    }

    /// **Stage 6a\'s gate: one interaction drives an agent.** The query finds the table, the
    /// allocation seats somebody, and the result says where they stand.
    #[test]
    fn an_agent_finds_a_table_and_sits_at_it() {
        let world = galley(4);
        let mut smart = SmartObjects::default();

        let offers = smart.offers(&world, Vec3::ZERO, 50.0);
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].verb, "eat");

        let cast = smart
            .start(
                &world,
                &offers[0].location,
                "eat",
                &diners(1),
                &mut emerge_core::rng::seeded(4),
            )
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cast.filled.len(), 1);
        let seat = cast.filled[0]
            .seat
            .as_ref()
            .unwrap_or_else(|| panic!("a diner sits somewhere"));
        assert_eq!(seat.prop, "t1");
    }

    /// **Stage 6b\'s gate, through the runtime: four agents fill a four-seat table.** No
    /// double-booking, and the table is busy afterwards.
    #[test]
    fn four_agents_fill_the_table_and_it_is_then_busy() {
        let world = galley(4);
        let mut smart = SmartObjects::default();
        let cast = smart
            .start(
                &world,
                "galley_table_1",
                "eat",
                &diners(4),
                &mut emerge_core::rng::seeded(4),
            )
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cast.filled.len(), 4);

        // Everyone is engaged and the location owns its props, so it offers nothing more.
        for id in 1..=4 {
            assert!(smart.is_engaged(id));
        }
        assert!(smart.is_busy("galley_table_1"));
        assert!(smart.offers(&world, Vec3::ZERO, 50.0).is_empty());

        // And it all comes back.
        assert!(smart.finish("galley_table_1").is_some());
        assert!(!smart.is_engaged(1));
        assert_eq!(smart.offers(&world, Vec3::ZERO, 50.0).len(), 1);
    }

    /// **A busy actor is not offered twice.** Two locations, four diners, and the second scene takes
    /// only whoever the first left — the exclusivity rule enforced by what the allocator is allowed
    /// to see rather than by a check afterwards.
    #[test]
    fn a_second_scene_takes_only_the_actors_the_first_left() {
        let mut world = galley(2);
        // A second table, so there is somewhere else to go.
        world.map.placements.push(placed("t2", "table"));
        let mut second = world.map.locations[0].clone();
        second.id = "galley_table_2".into();
        second.props = vec!["t2".into()];
        world.map.locations.push(second);
        let world = EmergeWorld::new(world.library, world.map, world.vocab)
            .unwrap_or_else(|e| panic!("{e}"));

        let mut smart = SmartObjects::default();
        let first = smart
            .start(
                &world,
                "galley_table_1",
                "eat",
                &diners(4),
                &mut emerge_core::rng::seeded(9),
            )
            .unwrap_or_else(|e| panic!("{e}"));
        let second = smart
            .start(
                &world,
                "galley_table_2",
                "eat",
                &diners(4),
                &mut emerge_core::rng::seeded(9),
            )
            .unwrap_or_else(|e| panic!("{e}"));

        let mut everyone: Vec<u64> = first.actors().chain(second.actors()).collect();
        let total = everyone.len();
        everyone.sort_unstable();
        everyone.dedup();
        assert_eq!(everyone.len(), total, "somebody is sitting at two tables");
        assert_eq!(total, 4, "two 2-seat tables should seat four");
    }

    /// The nearest thing wins, and equidistant ones are broken by a total order rather than by the
    /// order the map file happens to list them in.
    #[test]
    fn offers_come_back_nearest_first() {
        let mut world = galley(1);
        world.map.placements.push(Placed {
            at: (20.0, 0.0),
            ..placed("t2", "table")
        });
        let mut far = world.map.locations[0].clone();
        far.id = "galley_table_2".into();
        far.props = vec!["t2".into()];
        world.map.locations.push(far);
        let world = EmergeWorld::new(world.library, world.map, world.vocab)
            .unwrap_or_else(|e| panic!("{e}"));

        let smart = SmartObjects::default();
        let near = smart.offers(&world, Vec3::ZERO, 100.0);
        assert_eq!(near[0].location, "galley_table_1");
        // And the far one is out of range when the range is short.
        let close = smart.offers(&world, Vec3::ZERO, 5.0);
        assert_eq!(close.len(), 1);
    }

    /// A capability token the vocabulary does not hold is refused **at load**, naming the location and
    /// the role — not discovered as a scene that silently never starts.
    #[test]
    fn a_misspelled_capability_is_refused_at_load() {
        let mut world = galley(2);
        world.map.locations[0].interactions[0].roles[0].requires = vec!["eeat".into()];
        let err = EmergeWorld::new(world.library, world.map, world.vocab)
            .err()
            .unwrap_or_default();
        assert!(err.contains("galley_table_1/eat"), "{err}");
        assert!(err.contains("Did you mean `eat`?"), "{err}");
    }

        /// **A map is a thing you place.** `at` is map space and `origin` is where that space sits in the
    /// world, so the same authored room dropped in two corners of a level lands in two corners.
    ///
    /// The first version added only the origin's Y and used `at` directly for X and Z — correct for
    /// exactly one map, the one at the origin, which is the only one the editor ever authors.
    ///
    /// Y is not this function's to decide any more. It comes from `emerge_core::stack`, which is where
    /// the layer rules and their tests live — this only has to carry it through.
    #[test]
    fn a_map_origin_moves_the_whole_map_on_every_axis() {
        assert_eq!(
            origin_of((2.0, 3.0), (100.0, 5.0, -50.0), 5.0),
            Vec3::new(102.0, 5.0, -47.0)
        );
        // The offset is uniform: two pieces keep their spacing wherever the map goes.
        let a = origin_of((0.0, 0.0), (100.0, 0.0, -50.0), 0.0);
        let b = origin_of((4.0, 1.0), (100.0, 0.0, -50.0), 0.0);
        assert_eq!(b - a, Vec3::new(4.0, 0.0, 1.0));
    }
}
