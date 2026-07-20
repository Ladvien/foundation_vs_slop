//! `crab::setup` — crab + surface-graph + field spawning/rebuild (split out of the former `crab.rs`, 2026-07-19 review Finding: large files).

use super::*;

pub(crate) fn build_surface_graph(mut commands: Commands, dungeon: Res<Dungeon>) {
    let graph = SurfaceGraph::build(&dungeon);
    let (floor, wall) = graph.patch_stats();
    info!(
        "crab: surface graph built — {} patches ({} floor, {} wall)",
        graph.len(),
        floor,
        wall
    );
    commands.insert_resource(graph);
}

pub(crate) fn build_crab_anim(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    // glb clips: 0 = attack, 1 = idle, 2 = walk.
    let (graph, nodes) = AnimationGraph::from_clips([
        assets.load(GltfAssetLabel::Animation(0).from_asset(CRAB_GLB)),
        assets.load(GltfAssetLabel::Animation(1).from_asset(CRAB_GLB)),
        assets.load(GltfAssetLabel::Animation(2).from_asset(CRAB_GLB)),
    ]);
    let handle = graphs.add(graph);
    commands.insert_resource(CrabAnim {
        graph: handle,
        attack: nodes[0],
        idle: nodes[1],
        walk: nodes[2],
    });
}

/// Place `CRAB_CLUSTERS` nests in far rooms and fill each with crabs; the first `CRAB_WALL_CLUSTERS`
/// seed their crabs onto wall faces so climbing is visible from the start.
pub(crate) fn spawn_crabs(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    graph: Res<SurfaceGraph>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut nest_mats: ResMut<Assets<crate::nest::NestMaterial>>,
    mut seq: ResMut<CrabSpawnSeq>,
    sim: Res<SimTuning>,
    beh: Res<BehaviorTuning>,
) {
    let collider = meshes.add(Sphere::new(CRAB_COLLIDER_R));
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(CRAB_GLB));
    let dome = meshes.add(crate::nest::nest_dome_mesh()); // shared unit hemisphere → wall pimple per nest
    // Keep the shared handles so the reproduction system can birth new crabs at runtime.
    commands.insert_resource(CrabAssets {
        collider: collider.clone(),
        scene: scene.clone(),
    });

    // Greedily pick far, spread-apart nest seeds (deterministic, like `enemy::spawn_enemies`).
    let mut seeds: Vec<IVec2> = Vec::new();
    'scan: for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }
            if (cell - dungeon.spawn).as_vec2().length() < CRAB_MIN_SPAWN_DIST {
                continue;
            }
            if seeds
                .iter()
                .any(|c| (*c - cell).as_vec2().length() < CRAB_CLUSTER_SEP)
            {
                continue;
            }
            seeds.push(cell);
            if seeds.len() >= CRAB_CLUSTERS {
                break 'scan;
            }
        }
    }
    if seeds.is_empty() {
        warn!("crab: no floor cell far enough from spawn to place a nest");
        return;
    }

    // A dimensional nest portal near each cluster seed — the crabs' home + meat-delivery + birth anchor.
    // The dome sits ON a wall (bulging into the room); the delivery cell is the walled floor cell it
    // hangs over. Search the seed, then rings outward, for the nearest walled floor cell to seat it on.
    for &seed in &seeds {
        let mut placed = false;
        'search: for radius in 0i32..4 {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx.abs().max(dy.abs()) != radius {
                        continue; // only the shell of this ring (inner rings already tried)
                    }
                    let cell = seed + IVec2::new(dx, dy);
                    if !dungeon.is_floor(cell) {
                        continue;
                    }
                    let center = dungeon.cell_center(cell);
                    // Seat the nest only on a full-height wall. The camera-facing E/S edges are knee
                    // walls (squashed to `CAMERA_WALL_FRACTION`; their inner faces point -X / -Z — see
                    // `Dungeon::wall_faces_near`), and a dome seated mid-`WALL_HEIGHT` on one would
                    // float in the cutaway gap above the short wall. Prefer W/N faces (normals +X/+Z);
                    // a cell with only knee walls is skipped and the ring search moves on.
                    let full_face = dungeon.wall_faces_near(center).into_iter().find(|&(_, n)| {
                        !crate::dungeon::SHORT_CAMERA_WALLS || !crate::dungeon::is_camera_facing(n)
                    });
                    if let Some((face, normal)) = full_face {
                        if crate::nest::spawn_nest(
                            &mut commands,
                            &mut nest_mats,
                            dome.clone(),
                            face,
                            normal,
                            center,
                            &dungeon,
                        )
                        .is_some()
                        {
                            placed = true;
                            break 'search;
                        }
                    }
                }
            }
        }
        if !placed {
            warn!("crab: no wall face near cluster seed {seed:?} to seat a nest");
        }
    }

    let per_cluster = CRAB_COUNT.div_ceil(seeds.len());
    let ring = nest_offsets();
    let mut spawned = 0usize;

    for (ci, &seed) in seeds.iter().enumerate() {
        let on_wall = ci < CRAB_WALL_CLUSTERS;
        let mut in_cluster = 0usize;
        for &(dx, dy) in ring.iter() {
            if spawned >= CRAB_COUNT || in_cluster >= per_cluster {
                break;
            }
            let cell = seed + IVec2::new(dx, dy);
            if let Some(patch) = pick_patch(&graph, &dungeon, cell, on_wall) {
                let s = seq.0 as u32;
                seq.0 += 1;
                spawn_crab_on_patch(&mut commands, &graph, patch, &collider, &scene, s, &sim, beh.crab);
                spawned += 1;
                in_cluster += 1;
            }
        }
    }

    info!("crab: spawned {} crabs across {} nests", spawned, seeds.len());
}

/// Choose the patch a crab spawns on for `cell`: a wall face if `want_wall` and one exists, else the
/// cell's floor patch. Returns `None` if the cell is not a usable surface.
pub(crate) fn pick_patch(
    graph: &SurfaceGraph,
    dungeon: &Dungeon,
    cell: IVec2,
    want_wall: bool,
) -> Option<u32> {
    if !dungeon.is_floor(cell) {
        return None;
    }
    if want_wall {
        let center = dungeon.cell_center(cell);
        graph
            .wall_patch_at(dungeon, center)
            .or_else(|| graph.floor_patch_cell(cell))
    } else {
        graph.floor_patch_cell(cell)
    }
}

/// Spawn one crab seated on `patch`: an unscaled root (invisible collider + `Hostile`) with the scaled,
/// seated glTF model as a child.
pub(crate) fn spawn_crab_on_patch(
    commands: &mut Commands,
    graph: &SurfaceGraph,
    patch: u32,
    collider: &Handle<Mesh>,
    scene: &Handle<WorldAsset>,
    rand_seed: u32,
    sim: &SimTuning,
    bc: CrabTuning,
) {
    let p = graph.patch(patch);
    let pos = p.center;
    let normal = p.normal;
    let heading = p.tan_u;
    let seat = pos + normal * CRAB_BODY_CENTER;

    // Every per-crab random draw comes from the unique spawn seed, NOT the spawn position — bred crabs
    // share a delivery cell, so a position hash would clone them (see `CrabSpawnSeq`). Distinct salts
    // decorrelate the independent draws (role, capacity, jump cadence, biases).
    let draw = |salt: u32| hash01_u32(rand_seed.wrapping_mul(0x9E37_79B1).wrapping_add(salt));

    // ~bc.scout_fraction of crabs are scouts (recon recruiters); the rest run the assault brain. One path —
    // a plain conditional, no fallback.
    let is_scout = draw(1) < bc.scout_fraction;
    let brain_id = if is_scout {
        crate::ai::brain::BrainId::Scout
    } else {
        crate::ai::brain::BrainId::Crab
    };

    let mut ec = commands.spawn((
            Crab,
            Hostile,
            Health::new(sim.combat.crab_hp),
            NoHealthBar, // swarm chaff: no floating bar (40 would bury the screen)
            // Seed a per-crab starting HUNGER (salt 6, decorrelated from the other draws) so the swarm
            // begins differentiated — hungry crabs press to feed, sated ones forage — instead of a uniform
            // ramp where every crab hits HUNGER==1 in lockstep. Feeding sates it (`crab_feeding_sates_hunger`).
            crate::ai::drives::Drives::seeded(crate::ai::drives::DriveId::HUNGER, 0.2 + 0.6 * draw(6)),
            // Fear the squad's gunfire, never the swarm's own menace. Tagged here rather than at the two
            // call sites so runtime-bred crabs (`nest_reproduce`) inherit it too.
            crate::ai::faction::Faction::Crab,
            brain_id,
            crate::ai::brain::ActiveBehavior::new(rand_seed),
            crate::ai::brain::ThinkTimer::staggered(rand_seed),
            // Grouped so the spawn tuple stays within Bevy's 15-element Bundle limit. `Biological` rides
            // here (not as a 16th top-level element) — living flesh Almond Water can heal; tagged at spawn
            // so runtime-bred crabs (`nest_reproduce`) inherit it and no runtime archetype migration occurs.
            (
                Biological,
                // ~1 in 4 crabs can't smell the cyanide warning → walk into poison pools. On every crab.
                crate::health::CyanideSmell::from_seed(rand_seed as u64),
                CrabAttached { host: None },
                CrabCarry {
                    capacity: bc.carry_capacity * (0.8 + 0.4 * draw(2)),
                    target: None,
                    hauling: false,
                },
                CrabJump {
                    phase: JumpPhase::Ready,
                    timer: 0.0,
                    // Stagger initial cooldowns by seed so a fresh cluster doesn't pounce in lockstep.
                    cooldown: bc.jump_cooldown * draw(3),
                    from: Vec3::ZERO,
                    to: Vec3::ZERO,
                },
            ),
            CrabMotion {
                patch,
                pos,
                normal,
                heading,
                climb_bias: draw(4),
                angle_bias: draw(5),
                latch_rel: 0.0,
            },
            CrabState::Idle,
            // Sphere collider mesh paired with its CPU laser hit-volume (same radius, sphere = zero-height
            // capsule) so bolts test against the crab headlessly + deterministically.
            (
                Mesh3d(collider.clone()),
                crate::laser::LaserTarget {
                    radius: CRAB_COLLIDER_R,
                    half_height: 0.0,
                    id: crate::laser::target_id(crate::laser::TargetKind::Crab, rand_seed as u64),
                },
            ),
            // Render-only: smooth the crab's 60 Hz movement + surface rotation across the display refresh
            // (see `lib::run`). Grouped with `Transform` so the spawn tuple stays within Bevy's 15-element
            // Bundle limit.
            (
                Transform::from_translation(seat).with_rotation(surface_orientation(heading, normal)),
                // Component + plugin come from avian's `bevy_transform_interpolation` integration.
                avian3d::prelude::TransformInterpolation,
            ),
            Visibility::Inherited,
        ));
    ec.with_child((
        WorldAssetRoot(scene.clone()),
        Transform::from_translation(Vec3::Y * CRAB_MODEL_Y).with_scale(Vec3::splat(CRAB_RENDER_SCALE)),
    ));
    // Caste hysteresis timer + the immortal spawn seed, so `re_role_crabs` can flip this crab's role
    // deterministically as the swarm's needs shift (see that system's determinism note).
    ec.insert((Caste { cooldown: 0.0 }, CrabSeed(rand_seed)));
    // Crabs are photophobic — they steer down the `LightField` gradient toward shadow (see
    // `crab_locomotion` and `light::Photophobic`), so lit rooms become refuges and the swarm pools in the
    // dark. Added at spawn (stable archetype; `re_role_crabs` never touches it), so light response is a
    // fixed trait of the creature and can't churn the hashed sim actor's archetype at runtime.
    ec.insert(crate::light::Photophobic);
    // SCP-150 host state: a crab is also a parasitizable host (the three-body web — parasite ↔ crab ↔
    // squad). Always-present + inert until infested, added here so `nest_reproduce`'s bred crabs inherit
    // it too; a flipped field never splits the hashed crab archetype.
    ec.insert(crate::parasite::host_infestation_bundle());
    if is_scout {
        ec.insert(Scout::new(rand_seed));
    }
}

/// Cell offsets around a nest seed, sorted nearest-first, out to Chebyshev radius 3 (~49 cells) so a
/// cluster can fill even in a cramped room.
pub(crate) fn nest_offsets() -> Vec<(i32, i32)> {
    let mut v: Vec<(i32, i32)> = Vec::new();
    for dy in -3..=3 {
        for dx in -3..=3 {
            v.push((dx, dy));
        }
    }
    // SORT-OK: a fixed constant offset table, not an ECS query — the input order is source-code order.
    v.sort_by_key(|&(dx, dy)| dx * dx + dy * dy);
    v
}

/// Rebuild the shared surface field when the set of unit cells changes (copies
/// `enemy::rebuild_enemy_field`'s gate, over the surface graph).
pub(crate) fn rebuild_crab_field(
    graph: Option<Res<SurfaceGraph>>,
    units: Query<&Transform, With<Prey>>,
    dungeon: Res<Dungeon>,
    mut crab_field: ResMut<CrabField>,
) {
    let Some(graph) = graph else { return };

    let crab_field = &mut *crab_field;
    // `force` when the field isn't built yet, so a first run (or a graph that wasn't ready) still builds
    // even though the unit cells haven't moved — matches the old `&& field.is_some()` skip guard.
    let force = crab_field.field.is_none();
    crate::pathfind::rebuild_on_cell_change(
        units.iter().map(|t| dungeon.world_to_cell(t.translation)),
        &mut crab_field.last_cells,
        force,
        |cells| {
            let sources: Vec<u32> = cells
                .iter()
                .filter_map(|&c| graph.floor_patch_cell(c))
                .collect();
            crab_field.field = SurfaceField::build(&graph, &sources).map(Arc::new);
        },
    );
}

