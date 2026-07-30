//! Instantiating the generated dungeon as meshes: carpet floors, wallpaper walls, corner posts.
//! Cosmetic geometry spawned once per run (`RunBuild::Populate`).
//! Split out of the former single-file `dungeon.rs` (3,447 lines) — a **pure move**, no logic
//! changed, so the replay goldens are untouched (FVS-N-1). `use super::*` at the top of each submodule
//! inherits the parent's imports, which is what keeps the move mechanical and reviewable: the diff is
//! whole items relocated, not hundreds of rewritten `use` lines.

use super::*;

/// Instantiating the generated dungeon as meshes — carpet floors, wallpaper walls, corner posts.
///
/// **Separated from generation on purpose.** The `Dungeon` resource is the pinned simulation truth;
/// this plugin is one *rendering* of it. Swapping in a different art treatment means replacing this
/// plugin alone, and nothing that reads the grid (nav, fog, placement, containment) changes.
pub struct DungeonRenderPlugin;

impl Plugin for DungeonRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(crate::session::RunState::Active),
            spawn_tiles.in_set(crate::session::RunBuild::Populate),
        );
    }
}

/// Load a **non-colour** texture — a normal map or an ORM pack — with sRGB decoding switched off.
///
/// This is not a nicety. A normal map stores a unit vector per texel and an ORM pack stores three
/// scalars; neither is a picture. Load one through the default sRGB path and every channel is pushed
/// through the EOTF, which tilts normals toward +Z (relief flattens, and it flattens *most* where the
/// slope is steepest) and lifts roughness/occlusion off their authored values. The failure is subtle
/// enough to read as "the lighting is a bit off" rather than as a wrong-colour-space bug, so it is
/// worth a named helper rather than an easily-dropped closure at four call sites.
fn linear_texture(assets: &AssetServer, path: &'static str) -> Handle<Image> {
    assets.load_with_settings(path, |s: &mut ImageLoaderSettings| s.is_srgb = false)
}

/// Attach MikkTSpace tangents, without which the normal maps above are **silently ignored**.
///
/// Bevy gates normal mapping on the `VERTEX_TANGENTS` shader def, which is set from the mesh having
/// `ATTRIBUTE_TANGENT`. Bevy's primitive meshes (`Cuboid`, `Plane3d`) emit position/normal/UV and no
/// tangent, so without this every map wired above would load, cost memory, and change nothing on
/// screen — a failure with no error message anywhere.
///
/// Failure panics naming the site, the codebase's convention for a structurally-impossible invariant
/// (`sort_total!` does exactly this; `psi_vision::band_of` likewise). It cannot fire for these inputs —
/// generation needs an indexed triangle list with positions, normals and UVs, which every mesh here
/// has by construction — and `tangents_exist_for_every_dungeon_mesh` below pins that, so the branch is
/// dead rather than lurking.
fn with_tangents(mut mesh: Mesh, site: &str) -> Mesh {
    mesh.generate_tangents()
        .unwrap_or_else(|e| panic!("dungeon::render: tangent generation failed for {site}: {e}"));
    mesh
}

/// Instantiate the dungeon as textured primitives: a Backrooms-carpet floor quad per floor
/// cell, wallpaper cuboid walls on perimeter edges (corner pairs as clean two-cuboid Ls,
/// remaining single edges as straight walls). Tiles start hidden so fog reveals them.
pub(crate) fn spawn_tiles(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Shared materials + meshes (built once, reused for every tile). Both textures are
    // seamless, so the default sampler + [0,1] cuboid/plane UVs tile cleanly across cells.
    //
    // Each surface is diffuse + normal + ORM. The normal/ORM pair is what makes the relight land: an
    // irradiance environment map (`crate::world`) shades by surface normal, so perturbing the normal per
    // texel is what turns flat wallpaper into a surface. Under the old uniform `GlobalAmbientLight` these
    // maps would have changed nothing, which is why the project never had any.
    //
    // `perceptual_roughness` stays as the fallback factor Bevy multiplies the ORM's green channel by;
    // it is 1.0 here so the map alone decides, rather than the map being scaled by a second, invisible
    // 0.95 that would make every reading 5% smoother than the file says.
    // One surfacing helper, used for every biome × wall/floor × fog-state combination below, so a new
    // biome is a table row rather than another block of six near-identical field assignments.
    // `tint` is the fog dim (`None` = full brightness).
    let mut surface = |diffuse: &'static str,
                       normal: &'static str,
                       orm: &'static str,
                       tint: Option<Color>| {
        materials.add(StandardMaterial {
            base_color: tint.unwrap_or(Color::WHITE),
            base_color_texture: Some(assets.load(diffuse)),
            normal_map_texture: Some(linear_texture(&assets, normal)),
            metallic_roughness_texture: Some(linear_texture(&assets, orm)),
            occlusion_texture: Some(linear_texture(&assets, orm)),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            ..default()
        })
    };

    // Wall materials, indexed by `Biome as usize` — the same indexing `FloorMaterials::pick` uses.
    let wall_mats: [Handle<StandardMaterial>; 2] = [
        surface(WALL_TEXTURE, WALL_NORMAL_TEXTURE, WALL_ORM_TEXTURE, None),
        surface(CONCRETE_TEXTURE, CONCRETE_NORMAL_TEXTURE, CONCRETE_ORM_TEXTURE, None),
    ];
    // Floors, bright and dim. The dim twin is the "explored but not currently in a unit's line of
    // sight" fog state: `base_color` tints the texture, so a dark cool grey remembers the terrain
    // without lighting it up. The fog swaps floor tiles between the two (see `fog::apply_floor_fog`).
    let dim = Some(crate::palette::DUNGEON_STONE);
    let floor_bright: [Handle<StandardMaterial>; 2] = [
        surface(FLOOR_TEXTURE, FLOOR_NORMAL_TEXTURE, FLOOR_ORM_TEXTURE, None),
        surface(CONCRETE_TEXTURE, CONCRETE_NORMAL_TEXTURE, CONCRETE_ORM_TEXTURE, None),
    ];
    let floor_dim: [Handle<StandardMaterial>; 2] = [
        surface(FLOOR_TEXTURE, FLOOR_NORMAL_TEXTURE, FLOOR_ORM_TEXTURE, dim),
        surface(CONCRETE_TEXTURE, CONCRETE_NORMAL_TEXTURE, CONCRETE_ORM_TEXTURE, dim),
    ];
    // Tiles spawn bright; `fog::apply_floor_fog` dims them on the first visibility pass.
    commands.insert_resource(FloorMaterials {
        bright: floor_bright.clone(),
        dim: floor_dim,
    });

    let floor_mesh = meshes.add(with_tangents(
        Plane3d::default().mesh().size(TILE_SIZE, TILE_SIZE).into(),
        "floor",
    ));
    // One mesh per trimmed length — index by the number of ends [`edge_wall`] removed (0, 1 or 2). An
    // E/W slab is always index 0; only N/S slabs shorten, and only at a corner.
    let wall_meshes: [Handle<Mesh>; 3] = std::array::from_fn(|trims| {
        let len = TILE_SIZE - trims as f32 * WALL_THICKNESS;
        meshes.add(with_tangents(wall_mesh(Vec3::new(WALL_THICKNESS, WALL_HEIGHT, len)), "wall"))
    });
    // A corner post is a WALL_THICKNESS² column standing the full wall height, filling the vertex gap
    // where two perpendicular wall runs meet (see the post loop after the wall pass).
    let post_size = Vec3::new(WALL_THICKNESS, WALL_HEIGHT, WALL_THICKNESS);
    let wall_post = meshes.add(with_tangents(wall_mesh(post_size), "post"));

    // One static half-space at y=0 catches every gib chunk (the whole floor is at y=0), so we don't
    // need a physics collider per floor tile — only the gib-chunk physics world uses these (see `gore`).
    commands.spawn((
        crate::session::run_scoped(),
        RigidBody::Static,
        Collider::half_space(Vec3::Y),
        Transform::default(),
    ));

    // Walls get a static cuboid collider matching their mesh box, so gib chunks bounce off them and
    // stay in the room. `wall_size` is the box for walls, `None` for floor tiles (which need none).
    let mut spawn_tile = |cell: IVec2,
                          mesh: Handle<Mesh>,
                          material: Handle<StandardMaterial>,
                          mut transform: Transform,
                          wall_size: Option<Vec3>,
                          cutaway: Cutaway| {
        // The knee-wall cutaway is view-relative (see `update_cutaway`). We only *seed* the pose here for
        // the opening yaw=0 view (camera from +X,+Z ⇒ E/S near); the per-frame system re-poses it from
        // the live camera direction. A `Wall` (squashed) reseats to knee height on its near edge; a
        // `Mounted` decoration (doorway lintel) hides — scale 0 — on its near edge so it never floats.
        let outward = match cutaway {
            Cutaway::None => Vec3::ZERO,
            Cutaway::Wall | Cutaway::Mounted => {
                let center = Vec3::new(cell.x as f32 * TILE_SIZE, 0.0, cell.y as f32 * TILE_SIZE);
                wall_outward(transform.translation, center)
            }
            // A post sits on a vertex, not an edge, so its diagonal outward is supplied by the caller.
            Cutaway::Post(o) => o,
        };
        if SHORT_CAMERA_WALLS && faces_camera(outward, Vec3::new(1.0, 0.0, 1.0)) {
            match cutaway {
                Cutaway::Wall | Cutaway::Post(_) => {
                    let (scale_y, y) = wall_pose(true);
                    transform.scale.y = scale_y;
                    transform.translation.y = y;
                }
                Cutaway::Mounted => transform.scale = Vec3::ZERO,
                Cutaway::None => {}
            }
        }
        let mut entity = commands.spawn((
            Tile { cell },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            Visibility::Hidden,
        ));
        // Walls, lintels, and corner posts carry `Wall` so the fog reveal treats them as walls (not
        // floors); only solid walls/posts get a physics collider (lintels are cosmetic — gibs pass under
        // the ceiling beam).
        if matches!(
            cutaway,
            Cutaway::Wall | Cutaway::Mounted | Cutaway::Post(_)
        ) {
            entity.insert(Wall);
        }
        if let Some(size) = wall_size {
            // avian `Collider::cuboid` takes FULL side lengths; the wall mesh is an origin-centred
            // `Cuboid` of the same size, so the collider lines up exactly under the transform.
            entity.insert((RigidBody::Static, Collider::cuboid(size.x, size.y, size.z)));
        }
        match cutaway {
            Cutaway::Wall | Cutaway::Post(_) => {
                entity.insert(CutawayWall { outward });
            }
            Cutaway::Mounted => {
                entity.insert(CutawayMounted {
                    outward,
                    base_scale: Vec3::ONE,
                });
            }
            Cutaway::None => {}
        }
    };

    for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }

            spawn_tile(
                cell,
                floor_mesh.clone(),
                floor_bright[dungeon.biome(cell) as usize].clone(),
                Transform::from_translation(dungeon.cell_center(cell)),
                None,
                Cutaway::None,
            );

            // Which of this cell's edges border rock / off-grid → need a wall.
            let mut walled = [false; 4];
            for dir in [N, E, S, W] {
                walled[dir] = dungeon.walled(cell, dir);
            }

            // One slab per walled edge, sized by `edge_wall`'s single trim rule. No corner templates and
            // no greedy pair consumption, so a cell with three or four walled edges no longer
            // double-occupies a corner column.
            for dir in [N, E, S, W] {
                if !walled[dir] {
                    continue;
                }
                let (transform, size, trims) = edge_wall(cell, dir, walled);
                spawn_tile(
                    cell,
                    wall_meshes[trims].clone(),
                    wall_mats[dungeon.biome(cell) as usize].clone(),
                    transform,
                    Some(size),
                    Cutaway::Wall,
                );
            }
        }
    }

    // Corner posts: fill the WALL_THICKNESS² column left at a tile-corner vertex where two PERPENDICULAR
    // wall runs meet but the floor cell owning the junction contributes neither slab — a concave corner,
    // or a junction whose two walls come from different cells. Without this the two 0.14 m-thin inset
    // slabs meet at the shared vertex leaving a small empty post-shaped notch for the full wall height.
    // `corner_post` decides per quadrant (a vertex has four), so a post is inset flush with the walls it
    // joins instead of straddling the tile boundary. The `-1..dim` scan visits each vertex once.
    for cz in -1..dungeon.height as i32 {
        for cx in -1..dungeon.width as i32 {
            let vertex = IVec2::new(cx, cz);
            for quadrant in 0..VERTEX_QUADRANTS.len() {
                let Some((home, centre, outward)) = corner_post(&dungeon, vertex, quadrant) else {
                    continue;
                };
                spawn_tile(
                    home,
                    wall_post.clone(),
                    // `home` is the post's owning cell — the same key the wall runs it joins use, so a
                    // post never surfaces differently from the two walls meeting at it.
                    wall_mats[dungeon.biome(home) as usize].clone(),
                    Transform::from_translation(centre),
                    Some(post_size),
                    Cutaway::Post(outward),
                );
            }
        }
    }

    // Doorway lintels: a short wall header above each doorway (region opening), so the wall reads as one
    // continuous run from above — the door tucks under it. A doorway is now a band of `op.width` lanes
    // (necked down from its corridor's carved width), so a lintel is spawned over EVERY open lane —
    // stacked perpendicular to `op.dir` from lane 0, matching the necking geometry — instead of framing
    // only the centre lane and leaving a wide opening half-headed. Each lintel is raised to
    // `DOORWAY_HEIGHT`. A lintel is a `Cutaway::Mounted` decoration: it shows only while its wall is
    // far/full and hides (scale 0) when that wall becomes a near knee wall, so it never floats in the
    // cutaway gap. All four edges are spawned (the pose seeds E/S hidden at yaw=0); rotation reveals the
    // pair on whichever wall is currently full-height.
    let header_size = Vec3::new(WALL_THICKNESS, WALL_HEIGHT - DOORWAY_HEIGHT, TILE_SIZE);
    let header_mesh = meshes.add(with_tangents(wall_mesh(header_size), "doorway header"));
    for region in &dungeon.regions {
        for op in &region.openings {
            // Open lanes stack +y for an E/W mouth and +x for an N/S mouth — the same axis the necking
            // pass leaves open, so the header sits exactly over the cleared doorway lanes.
            for lane in 0..op.width as i32 {
                let cell = match op.dir {
                    E | W => IVec2::new(op.cell[0], op.cell[1] + lane),
                    N | S => IVec2::new(op.cell[0] + lane, op.cell[1]),
                    _ => IVec2::new(op.cell[0], op.cell[1]),
                };
                spawn_tile(
                    cell,
                    header_mesh.clone(),
                    wall_mats[dungeon.biome(cell) as usize].clone(),
                    header_wall(cell, op.dir),
                    None,
                    Cutaway::Mounted,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every mesh that carries a normal-mapped dungeon material must be able to produce tangents.
    ///
    /// This pins the `expect` in [`with_tangents`] dead, and it guards a failure mode with no runtime
    /// symptom: Bevy gates normal mapping on `ATTRIBUTE_TANGENT`, so a mesh that quietly lost its UVs
    /// or its indices would not warn — the normal and ORM maps would simply stop doing anything, and
    /// the surfaces would go back to looking flat with no error to grep for.
    #[test]
    fn tangents_exist_for_every_dungeon_mesh() {
        let floor: Mesh = Plane3d::default().mesh().size(TILE_SIZE, TILE_SIZE).into();
        let post = Vec3::new(WALL_THICKNESS, WALL_HEIGHT, WALL_THICKNESS);
        let header = Vec3::new(WALL_THICKNESS, WALL_HEIGHT - DOORWAY_HEIGHT, TILE_SIZE);
        let mut cases: Vec<(&str, Mesh)> =
            vec![("floor", floor), ("post", wall_mesh(post)), ("header", wall_mesh(header))];
        // Every trim variant, because they are separate meshes built at separate lengths.
        for trims in 0..3 {
            let len = TILE_SIZE - trims as f32 * WALL_THICKNESS;
            cases.push(("wall", wall_mesh(Vec3::new(WALL_THICKNESS, WALL_HEIGHT, len))));
        }
        for (name, mesh) in cases {
            let tangented = with_tangents(mesh, name);
            assert!(
                tangented.attribute(Mesh::ATTRIBUTE_TANGENT).is_some(),
                "{name}: tangent generation reported success but attached no ATTRIBUTE_TANGENT — the \
                 normal + ORM maps wired in spawn_tiles would be silently ignored"
            );
        }
    }
}
