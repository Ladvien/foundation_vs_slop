//! Room picking and shaping: weighted room-type selection, footprint draws, and notching.
//! Every draw goes through the seeded `DetRng`, never entropy.
//! Split out of the former single-file `dungeon.rs` (3,447 lines) — a **pure move**, no logic
//! changed, so the replay goldens are untouched (FVS-N-1). `use super::*` at the top of each submodule
//! inherits the parent's imports, which is what keeps the move mechanical and reviewable: the diff is
//! whole items relocated, not hundreds of rewritten `use` lines.

use super::*;

/// Pick a weighted room type and draw its footprint in tiles (Merrell 2011: per-room area + aspect).
/// Deterministic — draws type, area, aspect, then orientation from the carve RNG in a fixed order.
/// Dimensions round to whole tiles and clamp to `[ROOM_FLOOR, max_side]` so the room fits its block with
/// a rock margin. Returns `(width, depth, type_tag, expands)` — `expands` is the type's spacious flag,
/// which drives the all-edge expansion in `expand_to_fine`.
pub(crate) fn pick_room(
    config: &DungeonConfig,
    max_side: usize,
    rng: &mut impl DetRng,
) -> (usize, usize, String, bool) {
    let ty = weighted_room_type(&config.room_types, rng);
    let area = rand_range_f32(rng, ty.area_min, ty.area_max);
    let aspect = rand_range_f32(rng, ty.aspect_min, ty.aspect_max);
    let long = (area * aspect).sqrt();
    let short = (area / aspect).sqrt();
    // Randomly orient the long axis to x or y so rooms aren't all landscape.
    let (w_f, h_f) = if rng.unit() < 0.5 {
        (long, short)
    } else {
        (short, long)
    };
    let cap = max_side.max(ROOM_FLOOR); // guard clamp's min <= max even for a tiny block
    let rw = (w_f.round() as usize).clamp(ROOM_FLOOR, cap);
    let rh = (h_f.round() as usize).clamp(ROOM_FLOOR, cap);
    (rw, rh, ty.tag.clone(), ty.expands)
}

/// Cut rectangular bites from a filled room's corners, turning it into a rectilinear polygon (L / T / Z /
/// U / plus — 6 to 12 corners) by clearing cells in `walkable`. `(bx, by)` is the block-centre cell, which
/// is interior to the room; every notch stays strictly inside its own corner quadrant relative to
/// `(bx, by)`, so the centre cross (row `by`, column `bx`) is never cut — the room stays connected, the
/// block-centre corridor still meets floor, and `derive_opening` is unaffected. Deterministic: it draws
/// the chance roll, the corner count, a Fisher–Yates corner order (three swaps, always), then one depth per
/// cut — a fixed sequence for a given room size, so replays stay byte-stable.
#[allow(clippy::too_many_arguments)]
pub(crate) fn notch_room(
    walkable: &mut [bool],
    width: usize,
    ox: usize,
    oy: usize,
    rw: usize,
    rh: usize,
    bx: usize,
    by: usize,
    cfg: &NotchConfig,
    rng: &mut impl DetRng,
) {
    if rw.min(rh) < cfg.min_side || rng.unit() >= cfg.chance {
        return; // too small to shape, or the chance roll declined — stays a clean rectangle
    }
    let count = 1 + rng.below(cfg.max_corners); // 1..=max_corners distinct corners
                                                // Fisher–Yates over the four corners (0=NW,1=NE,2=SW,3=SE); always three swaps so the draw count is
                                                // independent of `count`, then take the first `count`.
    let mut order = [0usize, 1, 2, 3];
    for i in (1..4).rev() {
        order.swap(i, rng.below(i + 1));
    }
    for &corner in order.iter().take(count) {
        let depth = cfg.depth_min + rng.unit() as f32 * (cfg.depth_max - cfg.depth_min);
        let (is_left, is_top) = (corner == 0 || corner == 2, corner == 0 || corner == 1);
        // Available quadrant between the room edge and the centre cross (exclusive of the centre row/col).
        let avail_w = if is_left { bx - ox } else { (ox + rw - 1) - bx };
        let avail_h = if is_top { by - oy } else { (oy + rh - 1) - by };
        if avail_w == 0 || avail_h == 0 {
            continue; // room hugs the centre on this side — no room for a corner bite here
        }
        let nw = ((depth * avail_w as f32).round() as usize).clamp(1, avail_w);
        let nh = ((depth * avail_h as f32).round() as usize).clamp(1, avail_h);
        let x0 = if is_left { ox } else { ox + rw - nw };
        let y0 = if is_top { oy } else { oy + rh - nh };
        for y in y0..y0 + nh {
            for x in x0..x0 + nw {
                walkable[y * width + x] = false;
            }
        }
    }
}

/// Weighted choice of a room type (same idiom as `wfc::collapse_grid`'s prototype pick). `types` is
/// validated non-empty with positive total weight at config load, so the fall-through is unreachable.
fn weighted_room_type<'a>(types: &'a [RoomType], rng: &mut impl DetRng) -> &'a RoomType {
    let total: f64 = types.iter().map(|t| t.weight).sum();
    let mut r = rng.unit() * total;
    for t in types {
        r -= t.weight;
        if r <= 0.0 {
            return t;
        }
    }
    &types[types.len() - 1]
}

/// Uniform f32 in `[lo, hi)` from the deterministic RNG; returns `lo` for a degenerate/inverted range.
fn rand_range_f32(rng: &mut impl DetRng, lo: f32, hi: f32) -> f32 {
    if hi <= lo {
        return lo;
    }
    lo + (rng.unit() as f32) * (hi - lo)
}

/// Slide a room's centred origin off-centre within its block for the liminality dial (Phase 2). `t` is
/// `1 - liminality`: at `t = 0` (liminality 1.0) the room stays centred — the shipped Backrooms grid,
/// and *no RNG is drawn*, so that layout is byte-identical to the pre-dial carve. As `t` grows the room
/// slides by up to `t` of its available slack, chosen from the carve RNG. The slack is bounded by two
/// rules: keep a >=1-tile rock margin inside the block, and keep the block centre (`bcenter`) at least
/// one cell inside the room walls — so the block-centre corridor lane is always interior floor and every
/// corridor still connects, with no change to the corridor carve. Rooms stay axis-aligned rectangles.
pub(crate) fn jitter_origin(
    centered: usize,
    room: usize,
    block_start: usize,
    block: usize,
    bcenter: usize,
    t: f32,
    rng: &mut impl DetRng,
) -> usize {
    if t <= 0.0 {
        return centered;
    }
    let (c, r, bs, blk, bc) = (
        centered as i64,
        room as i64,
        block_start as i64,
        block as i64,
        bcenter as i64,
    );
    let lo = (bs + 1).max(bc - r + 2);
    let hi = (bs + blk - r - 1).min(bc - 1);
    if hi <= lo {
        return centered; // no slack (a small room hugging the block centre) → stay centred
    }
    // Window of half-widths `t·(c-lo)` left and `t·(hi-c)` right, both within [lo, hi] since t <= 1.
    let left = (((c - lo) as f32) * t).round() as i64;
    let right = (((hi - c) as f32) * t).round() as i64;
    ((c - left) + rng.below((left + right + 1) as usize) as i64) as usize
}

/// Flood-fill the coarse room slots across Link edges, returning a per-slot mask of the
/// single largest connected component (the playable dungeon; the rest becomes rock).
pub(crate) fn largest_room_component(coarse: &wfc::WfcResult) -> Vec<bool> {
    let (w, h) = (coarse.width, coarse.height);
    let is_room = |i: usize| coarse.cells[i].kind == CellKind::Floor;
    let mut visited = vec![false; w * h];
    let mut best: Vec<usize> = Vec::new();

    for start in 0..w * h {
        if visited[start] || !is_room(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        visited[start] = true;
        while let Some(i) = stack.pop() {
            component.push(i);
            let (cx, cy) = (i % w, i / w);
            // A Link edge on this slot connects to a room neighbour (socket rule).
            let links = [
                (cy > 0, N, i.wrapping_sub(w)),
                (cx + 1 < w, E, i + 1),
                (cy + 1 < h, S, i + w),
                (cx > 0, W, i.wrapping_sub(1)),
            ];
            for (in_bounds, dir, ni) in links {
                if in_bounds && coarse.cells[i].open[dir] && is_room(ni) && !visited[ni] {
                    visited[ni] = true;
                    stack.push(ni);
                }
            }
        }
        if component.len() > best.len() {
            best = component;
        }
    }

    let mut kept = vec![false; w * h];
    for i in best {
        kept[i] = true;
    }
    kept
}

/// Transform for a full-length straight wall cuboid on edge `dir` of cell `c`. The cuboid
/// is inset by its half-thickness to sit *flush* with the tile edge (outer face on ±0.5),
/// and lifted by half its height (Bevy cuboids are origin-centred) so it rests on the floor.
fn straight_wall(c: IVec2, dir: usize) -> Transform {
    let (i, j) = (c.x as f32, c.y as f32);
    let quarter = std::f32::consts::FRAC_PI_2;
    let h = WALL_HALF_THICKNESS;
    let y = WALL_HEIGHT * 0.5;
    match dir {
        // Vertical wall lines run along Z (the cuboid's long axis).
        E => Transform::from_xyz((i + 0.5) * TILE_SIZE - h, y, j * TILE_SIZE),
        W => Transform::from_xyz((i - 0.5) * TILE_SIZE + h, y, j * TILE_SIZE),
        // Horizontal wall lines run along X → rotate the cuboid 90°.
        S => Transform::from_xyz(i * TILE_SIZE, y, (j + 0.5) * TILE_SIZE - h)
            .with_rotation(Quat::from_rotation_y(quarter)),
        N => Transform::from_xyz(i * TILE_SIZE, y, (j - 0.5) * TILE_SIZE + h)
            .with_rotation(Quat::from_rotation_y(quarter)),
        _ => unreachable!(),
    }
}

/// Transform for a doorway lintel: a straight wall on edge `dir` of cell `c`, but raised so its
/// bottom sits at [`DOORWAY_HEIGHT`] and it fills up to the ceiling — the header above a door.
pub(crate) fn header_wall(c: IVec2, dir: usize) -> Transform {
    let base = straight_wall(c, dir);
    let y = DOORWAY_HEIGHT + (WALL_HEIGHT - DOORWAY_HEIGHT) * 0.5;
    base.with_translation(base.translation.with_y(y))
}

/// Transform, full box size, and trimmed-end count for the wall slab on edge `dir` of cell `c`, given
/// which of that cell's four edges are walled. **One rule for every case** — no corner templates, no
/// greedy pair consumption:
///
/// * an **E/W** slab always runs the full tile length along Z;
/// * an **N/S** slab is **trimmed** by [`WALL_THICKNESS`] at each end whose perpendicular (E/W) edge is
///   also walled, so it stops exactly at that slab's inner face.
///
/// The asymmetry is what makes the set watertight: at a corner the E/W slab owns the shared
/// `WALL_THICKNESS²` column and the N/S slab yields it. Over all 16 subsets of `walled` the slabs are
/// pairwise disjoint (no coincident faces → no z-fighting on textured corners) and leave no gap; see
/// `walls_of_a_cell_never_overlap_and_leave_no_gap`. The predecessor consumed adjacent walled *pairs* as
/// L-shaped corner arms and left any remaining edge at full length, so every cell with three or four
/// walled edges — dead-end corridor caps, notched room corners — double-occupied a corner column.
///
/// The returned count (0, 1 or 2) is how many ends were removed; it indexes the caller's pre-built mesh
/// set, so the trim rule lives in exactly one place.
pub(crate) fn edge_wall(c: IVec2, dir: usize, walled: [bool; 4]) -> (Transform, Vec3, usize) {
    let base = straight_wall(c, dir);
    match dir {
        E | W => (base, Vec3::new(WALL_THICKNESS, WALL_HEIGHT, TILE_SIZE), 0),
        N | S => {
            let trim_w = if walled[W] { WALL_THICKNESS } else { 0.0 };
            let trim_e = if walled[E] { WALL_THICKNESS } else { 0.0 };
            let trims = walled[W] as usize + walled[E] as usize;
            // The N/S cuboid is rotated a quarter turn, so its long axis is world X: shorten it along Z
            // (pre-rotation) and slide it in world X toward whichever end was NOT trimmed.
            let transform = Transform {
                translation: base.translation + Vec3::X * ((trim_w - trim_e) * 0.5),
                ..base
            };
            (
                transform,
                Vec3::new(WALL_THICKNESS, WALL_HEIGHT, TILE_SIZE - trim_w - trim_e),
                trims,
            )
        }
        _ => unreachable!(),
    }
}

/// The four cells meeting at a tile vertex, as `(cell offset from the vertex, v_edge, h_edge)`: the vertex
/// is that cell's corner, `v_edge` is the cell's vertical (E/W) edge there and `h_edge` its horizontal
/// (N/S) one. Order: NW cell, NE, SW, SE.
pub(crate) const VERTEX_QUADRANTS: [(IVec2, usize, usize); 4] = [
    (IVec2::new(0, 0), E, S),
    (IVec2::new(1, 0), W, S),
    (IVec2::new(0, 1), E, N),
    (IVec2::new(1, 1), W, N),
];

/// The corner post (if any) that closes `quadrant` of the tile vertex whose NW cell is `vertex` — the
/// `WALL_THICKNESS²` column left where a vertical wall run meets a horizontal one but the floor cell
/// owning the junction contributes neither slab. Returns `(home cell, centre, outward)`; `home` owns the
/// post for the fog reveal (always a floor cell, never rock) and `outward` is the diagonal the knee-wall
/// cutaway squashes along.
///
/// The post is **inset exactly like every wall**, never centred on the vertex. A wall's outer face sits on
/// the tile boundary, so a vertex-centred post pushes half its width straight through that face into the
/// void and fills only a quarter of the gap it exists to close — the player-reported "the walls build off
/// origin at one side, but the corners align in the centre … half of the corner pieces poke through the
/// wall" (debug capture 2026-07-24). Which side each axis insets to follows the cell that owns the
/// adjacent slab, not which cells are floor; the two disagree at a diagonal pinch.
///
/// A corner needs no post when the owning cell walls it itself: an E/W slab spans the full tile length,
/// and an N/S slab is trimmed only at an end whose perpendicular E/W edge is walled (see [`edge_wall`]),
/// so *either* of the cell's own edges at the vertex being walled already covers the column.
pub(crate) fn corner_post(dungeon: &Dungeon, vertex: IVec2, quadrant: usize) -> Option<(IVec2, Vec3, Vec3)> {
    let (offset, v_edge, h_edge) = VERTEX_QUADRANTS[quadrant];
    let cell = vertex + offset;
    if !dungeon.is_floor(cell) || dungeon.walled(cell, v_edge) || dungeon.walled(cell, h_edge) {
        return None;
    }
    // Both runs must actually continue past the vertex, else there is no junction to close: the vertical
    // run beyond this cell's horizontal edge, and the horizontal run beyond its vertical edge. `walled` is
    // false off-grid and on rock, so boundary vertices need no special case.
    if !dungeon.walled(Dungeon::neighbor(cell, h_edge), v_edge)
        || !dungeon.walled(Dungeon::neighbor(cell, v_edge), h_edge)
    {
        return None;
    }
    // `v_edge == E` ⇒ the cell lies west of the vertex, so its corner column is the WALL_THICKNESS band
    // just west of it (centre one half-thickness back), and the rock is east ⇒ outward +X. Likewise for Z.
    let (dx, out_x) = if v_edge == E { (-WALL_HALF_THICKNESS, 1.0) } else { (WALL_HALF_THICKNESS, -1.0) };
    let (dz, out_z) = if h_edge == S { (-WALL_HALF_THICKNESS, 1.0) } else { (WALL_HALF_THICKNESS, -1.0) };
    let centre = Vec3::new(
        (vertex.x as f32 + 0.5) * TILE_SIZE + dx,
        WALL_HEIGHT * 0.5,
        (vertex.y as f32 + 0.5) * TILE_SIZE + dz,
    );
    Some((cell, centre, Vec3::new(out_x, 0.0, out_z)))
}

/// Build a wall cuboid whose wallpaper stands upright on every side face. Bevy's default
/// `Cuboid` UVs lay the texture on its side on the ±X faces, so straight walls and full corner
/// arms (which show their ±X faces) render the Backrooms stripes/chevrons running horizontally.
/// Here every side face maps the texture's V axis to world +Y, so the pattern is vertical
/// regardless of which way the wall faces (Y-axis rotations keep "up" as up).
pub(crate) fn wall_mesh(size: Vec3) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::new(size.x, size.y, size.z));
    let (
        Some(VertexAttributeValues::Float32x3(positions)),
        Some(VertexAttributeValues::Float32x3(normals)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION).cloned(),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL).cloned(),
    )
    else {
        return mesh;
    };
    let half = size * 0.5;
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .zip(normals.iter())
        .map(|(p, n)| {
            let p = Vec3::from_array(*p);
            let n = Vec3::from_array(*n);
            if n.y.abs() > 0.5 {
                // Top / bottom faces: floor-plane mapping (their orientation is barely seen).
                [(p.x + half.x) / size.x, (p.z + half.z) / size.z]
            } else {
                // Side face: V climbs with world height so the wallpaper is upright; U runs
                // along the face's horizontal edge (Z for the ±X faces, X for the ±Z faces).
                let u = if n.x.abs() > 0.5 {
                    (p.z + half.z) / size.z
                } else {
                    (p.x + half.x) / size.x
                };
                let v = (half.y - p.y) / size.y; // V=0 at the top → texture right-way-up
                [u, v]
            }
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh
}
