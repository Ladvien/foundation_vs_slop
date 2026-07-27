//! Dungeon generation tests — moved wholesale out of the former single-file `dungeon.rs`.

use super::*;

/// World-space AABB of a wall cuboid: `size` is the pre-rotation box, and the only rotations used are
/// identity (E/W) or a quarter turn about Y (N/S), which swaps the X and Z extents.
fn wall_aabb(t: &Transform, size: Vec3) -> (Vec3, Vec3) {
    let rotated = (t.rotation * size).abs(); // quarter turns keep it axis-aligned
    let half = rotated * 0.5;
    (t.translation - half, t.translation + half)
}

/// Do two AABBs share interior volume? Touching faces (`a.max == b.min`) are fine — that is exactly how
/// abutting slabs are meant to meet; only genuine overlap double-occupies a column.
fn aabbs_overlap(a: (Vec3, Vec3), b: (Vec3, Vec3)) -> bool {
    const EPS: f32 = 1e-4;
    (0..3).all(|i| a.0[i] + EPS < b.1[i] && b.0[i] + EPS < a.1[i])
}

/// [`edge_wall`]'s single trim rule is watertight for **all 16** wall subsets of a cell: the slabs
/// never overlap (no coincident faces → no z-fighting) and together they cover every walled edge band
/// including its two corner columns.
///
/// This is the regression for the player-reported corner artifact (debug capture 2026-07-24). The
/// predecessor consumed adjacent walled *pairs* as L-shaped corner arms and left any third/fourth edge
/// at full tile length, so e.g. a cell walled N+E+W double-occupied `x ∈ [−0.50,−0.36] ×
/// z ∈ [−0.50,−0.36]`, and a fully-walled cell double-occupied two such columns.
#[test]
fn walls_of_a_cell_never_overlap_and_leave_no_gap() {
    let cell = IVec2::new(3, 5);
    let centre = Vec3::new(cell.x as f32 * TILE_SIZE, 0.0, cell.y as f32 * TILE_SIZE);

    for mask in 0u8..16 {
        let mut walled = [false; 4];
        for (dir, w) in walled.iter_mut().enumerate() {
            *w = mask & (1 << dir) != 0;
        }
        let boxes: Vec<(Vec3, Vec3)> = [N, E, S, W]
            .into_iter()
            .filter(|&dir| walled[dir])
            .map(|dir| {
                let (t, size, _) = edge_wall(cell, dir, walled);
                wall_aabb(&t, size)
            })
            .collect();

        // (1) Pairwise disjoint.
        for i in 0..boxes.len() {
            for j in (i + 1)..boxes.len() {
                assert!(
                    !aabbs_overlap(boxes[i], boxes[j]),
                    "walled={walled:?}: slabs {i} and {j} overlap ({:?} vs {:?})",
                    boxes[i],
                    boxes[j]
                );
            }
        }

        // (2) No gap: every walled edge's band — corner columns included — is covered by some slab.
        // Probe the two corner columns and the middle of each walled edge.
        for dir in [N, E, S, W] {
            if !walled[dir] {
                continue;
            }
            // Sample the slab's centre line at both ends and the middle. The end samples sit at
            // ±(half tile − half thickness), i.e. strictly *inside* the two corner columns — the
            // squares an E/W slab must own and an N/S slab must yield. A boundary sample would pass
            // vacuously; these fail if either column is left bare.
            let depth = 0.5 * TILE_SIZE - WALL_HALF_THICKNESS; // slab centre-line offset
            let along = [-depth, 0.0, depth];
            let probes: Vec<Vec3> = along
                .iter()
                .map(|&a| match dir {
                    N => centre + Vec3::new(a, WALL_HEIGHT * 0.5, -depth),
                    S => centre + Vec3::new(a, WALL_HEIGHT * 0.5, depth),
                    E => centre + Vec3::new(depth, WALL_HEIGHT * 0.5, a),
                    _ => centre + Vec3::new(-depth, WALL_HEIGHT * 0.5, a),
                })
                .collect();
            for p in probes {
                assert!(
                    boxes.iter().any(|b| (0..3).all(|i| p[i] >= b.0[i] - 1e-4 && p[i] <= b.1[i] + 1e-4)),
                    "walled={walled:?}: point {p:?} on edge {dir} is not covered by any slab"
                );
            }
        }
    }
}

/// [`corner_post`] over **all 16** floor/rock arrangements of the four cells around a vertex: every
/// post it returns sits flush inside its home floor cell (never straddling the tile boundary, which is
/// the "half of the corner pieces poke through the wall" bug), never overlaps a wall slab, and appears
/// exactly where a junction is genuinely bare.
#[test]
fn corner_posts_are_inset_flush_and_never_overlap_a_wall() {
    // A 4×4 grid whose middle 2×2 (cells (1,1)..(2,2)) is set from the mask; the outer ring stays rock
    // so the vertex under test is the one between them, at IVec2::new(1, 1).
    let vertex = IVec2::new(1, 1);
    let quadrant_cells = [IVec2::new(1, 1), IVec2::new(2, 1), IVec2::new(1, 2), IVec2::new(2, 2)];

    for mask in 0u8..16 {
        let mut walkable = vec![false; 16];
        for (bit, c) in quadrant_cells.iter().enumerate() {
            if mask & (1 << bit) != 0 {
                walkable[(c.y * 4 + c.x) as usize] = true;
            }
        }
        let d = Dungeon::from_walkable(4, 4, walkable);

        // Every wall slab in the whole grid, so a post can be checked against all of them.
        let mut slabs: Vec<(Vec3, Vec3)> = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                let cell = IVec2::new(x, y);
                if !d.is_floor(cell) {
                    continue;
                }
                let mut walled = [false; 4];
                for dir in [N, E, S, W] {
                    walled[dir] = d.walled(cell, dir);
                }
                for dir in [N, E, S, W] {
                    if walled[dir] {
                        let (t, size, _) = edge_wall(cell, dir, walled);
                        slabs.push(wall_aabb(&t, size));
                    }
                }
            }
        }

        let post_size = Vec3::new(WALL_THICKNESS, WALL_HEIGHT, WALL_THICKNESS);
        for quadrant in 0..VERTEX_QUADRANTS.len() {
            let Some((home, centre, outward)) = corner_post(&d, vertex, quadrant) else {
                continue;
            };
            let post = wall_aabb(&Transform::from_translation(centre), post_size);

            assert!(d.is_floor(home), "mask={mask:#06b} q={quadrant}: post home {home:?} is not floor");
            // Flush, not straddling: the post lies wholly inside its home cell's tile square.
            let lo = Vec3::new(
                (home.x as f32 - 0.5) * TILE_SIZE,
                0.0,
                (home.y as f32 - 0.5) * TILE_SIZE,
            );
            let hi = Vec3::new(
                (home.x as f32 + 0.5) * TILE_SIZE,
                WALL_HEIGHT,
                (home.y as f32 + 0.5) * TILE_SIZE,
            );
            for i in 0..3 {
                assert!(
                    post.0[i] >= lo[i] - 1e-4 && post.1[i] <= hi[i] + 1e-4,
                    "mask={mask:#06b} q={quadrant}: post {post:?} pokes outside home cell {home:?}"
                );
            }
            // And it fills the notch rather than double-occupying a slab.
            for s in &slabs {
                assert!(
                    !aabbs_overlap(post, *s),
                    "mask={mask:#06b} q={quadrant}: post {post:?} overlaps wall slab {s:?}"
                );
            }
            // `outward` is the floor→rock diagonal: both components are unit signs.
            assert_eq!(outward.y, 0.0);
            assert!(outward.x.abs() == 1.0 && outward.z.abs() == 1.0, "outward {outward:?}");
        }
    }
}

/// The concave corner, spelled out: floor at (0,0), (1,0), (0,1) with (1,1) rock leaves a bare
/// `0.14²` notch at the (0,0) cell's SE corner — centre `(0.43, 0.43)`, NOT the tile vertex `(0.5,
/// 0.5)` the old code used. Exactly one post closes it.
#[test]
fn concave_corner_post_sits_at_the_notch_not_the_vertex() {
    let d = Dungeon::from_walkable(3, 3, {
        let mut w = vec![false; 9];
        for c in [IVec2::new(0, 0), IVec2::new(1, 0), IVec2::new(0, 1)] {
            w[(c.y * 3 + c.x) as usize] = true;
        }
        w
    });
    let posts: Vec<_> = (0..VERTEX_QUADRANTS.len())
        .filter_map(|q| corner_post(&d, IVec2::new(0, 0), q))
        .collect();
    assert_eq!(posts.len(), 1, "one post closes a concave corner, got {posts:?}");
    let (home, centre, outward) = posts[0];
    assert_eq!(home, IVec2::new(0, 0));
    let expect = 0.5 * TILE_SIZE - WALL_HALF_THICKNESS; // 0.43, flush with both slabs' inner faces
    assert!((centre.x - expect).abs() < 1e-5, "post x {} != {expect}", centre.x);
    assert!((centre.z - expect).abs() < 1e-5, "post z {} != {expect}", centre.z);
    assert_eq!(outward, Vec3::new(1.0, 0.0, 1.0), "outward points SE, into the rock cell");
}

/// The doorway keep-clear reject (`footprint_clears_openings`): a footprint parked in a doorway's
/// approach band is rejected; one clear of it is accepted; and a corridor mouth on the +X wall neans
/// the band lies just inside the room, not out in the corridor. Uses a hand-built `Opening` (the
/// method reads only the opening geometry + `TILE_SIZE`, not the walkable mask).
#[test]
fn footprint_clears_openings_rejects_doorway_and_accepts_clear() {
    // Minimal dungeon — the method doesn't consult the walkable mask, only the openings passed in.
    let d = Dungeon::from_walkable(1, 1, vec![true]);
    // A 1-lane doorway piercing the +X (east) wall of interior cell (5,5): the keep-clear band is
    // x ∈ [5.5 − keep_clear, 5.5], z ∈ [4.5, 5.5].
    let openings = [Opening { dir: E, cell: [5, 5], width: 1 }];
    let keep = 1.1;
    let half = Vec2::splat(0.3);
    // Sitting in the mouth (centre on the doorway cell) → overlaps the band → NOT clear.
    assert!(
        !d.footprint_clears_openings(Vec3::new(5.0, 0.0, 5.0), half, 0.0, &openings, keep),
        "a piece in the doorway must be rejected"
    );
    // Two tiles into the room (well past the 1.1 m band) → clear.
    assert!(
        d.footprint_clears_openings(Vec3::new(3.0, 0.0, 5.0), half, 0.0, &openings, keep),
        "a piece clear of the doorway approach must be accepted"
    );
    // Off to the side of the doorway lane (different z) → clear.
    assert!(
        d.footprint_clears_openings(Vec3::new(5.0, 0.0, 7.0), half, 0.0, &openings, keep),
        "a piece beside the doorway lane must be accepted"
    );
}

/// Corridor identity is a partition of floor: every cell is room floor XOR corridor floor, never both,
/// and rock is neither. This is the invariant `mycelia::habitat` leans on to keep patches out of halls.
#[test]
fn corridor_cells_are_floor_and_never_room_floor() {
    let d = Dungeon::generate(&test_config()).expect("test config generates");
    let mut corridor_cells = 0usize;
    for y in 0..d.height as i32 {
        for x in 0..d.width as i32 {
            let c = IVec2::new(x, y);
            match d.corridor_id(c) {
                Some(_) => {
                    assert!(d.is_floor(c), "corridor cell {c:?} must be floor");
                    corridor_cells += 1;
                }
                None => {}
            }
            // Rock is never a corridor, whatever the carve left painted underneath it.
            if !d.is_floor(c) {
                assert!(!d.is_corridor(c), "rock {c:?} reported as corridor");
            }
        }
    }
    assert!(corridor_cells > 0, "a connected dungeon must have corridor floor");
}

/// The corridor painting must be a pure function of the seed, like every other carve decision.
#[test]
fn corridor_identity_is_deterministic() {
    let a = Dungeon::generate(&test_config()).expect("generates");
    let b = Dungeon::generate(&test_config()).expect("generates");
    assert_eq!(a.corridor_of, b.corridor_of, "same seed must paint the same runs");
}

/// A small, valid config for generation tests (avoids depending on the shipped RON's exact values).
fn test_config() -> DungeonConfig {
    DungeonConfig {
        coarse_w: 4,
        coarse_h: 4,
        block: 16,
        corridor_width: 2,
        corridor_width_max: Some(4),
        doorway_ratio: 0.5,
        seed: 0x5C0_9191,
        max_attempts: 20,
        liminality: 1.0,
        wfc_weights: WfcWeights {
            rock: 6.0,
            dead_end: 1.2,
            corridor: 2.5,
            corner: 2.5,
            tee: 1.2,
            cross: 0.6,
        },
        room_types: vec![
            RoomType {
                tag: "bathroom".into(),
                area_min: 3.0,
                area_max: 6.0,
                aspect_min: 1.0,
                aspect_max: 1.6,
                weight: 0.8,
                expands: false,
            },
            RoomType {
                tag: "bedroom".into(),
                area_min: 9.0,
                area_max: 20.0,
                aspect_min: 1.0,
                aspect_max: 1.5,
                weight: 1.5,
                expands: false,
            },
            RoomType {
                tag: "living".into(),
                area_min: 16.0,
                area_max: 40.0,
                aspect_min: 1.0,
                aspect_max: 1.7,
                weight: 1.6,
                expands: true,
            },
        ],
        notch: None,
        topology: Topology::Grid,
    }
}

#[test]
fn shipped_config_parses_and_generates() {
    // The shipped assets/config/config.ron `dungeon:` slice must parse, validate, and generate a
    // non-empty dungeon (loaded + validated through the unified loader, one path).
    let config = crate::config::load_game_config()
        .expect("shipped config.ron must be valid")
        .dungeon;
    let d = Dungeon::generate(&config).expect("must generate at least one room");
    assert!(!d.regions.is_empty(), "shipped config must produce rooms");
    assert!(d.walkable.iter().any(|&w| w), "dungeon must have floor");
}

#[test]
fn notching_carves_deterministic_non_rectangular_rooms() {
    // Rooms big enough to notch: one spacious type that fills its block at liminality 0, with notching
    // forced on for all four corners. Every room should become a rectilinear polygon (a plus/cross).
    let mut cfg = test_config();
    cfg.liminality = 0.0;
    cfg.room_types = vec![RoomType {
        tag: "hall".into(),
        area_min: 60.0,
        area_max: 120.0,
        aspect_min: 1.0,
        aspect_max: 1.4,
        weight: 1.0,
        expands: true,
    }];
    cfg.notch = Some(NotchConfig {
        chance: 1.0,
        max_corners: 4,
        depth_min: 0.4,
        depth_max: 0.5,
        min_side: 4,
    });

    let a = Dungeon::generate(&cfg).expect("gen a");
    let b = Dungeon::generate(&cfg).expect("gen b");
    assert_eq!(
        a.walkable, b.walkable,
        "notching must be deterministic for a (config, seed)"
    );

    // A plain rectangle has every bounding-box cell as floor; a notched room has non-floor bites in
    // its bbox. At least one room must be non-rectangular.
    let non_rect = a.regions.iter().filter(|r| {
        let (mn, mx) = (r.rect.min, r.rect.max);
        (mn[1]..mx[1]).any(|y| (mn[0]..mx[0]).any(|x| !a.is_floor(IVec2::new(x, y))))
    });
    assert!(
        non_rect.count() > 0,
        "expected at least one notched (non-rectangular) room"
    );
}

#[test]
fn notching_never_severs_a_room_from_its_corridors() {
    // The notch invariant: the block-centre cross is never cut, so every walkable cell stays reachable.
    // A full flood-fill from the spawn must still cover every floor cell (no notch orphans a region).
    let mut cfg = test_config();
    cfg.notch = Some(NotchConfig {
        chance: 1.0,
        max_corners: 4,
        depth_min: 0.4,
        depth_max: 0.6,
        min_side: 4,
    });
    let d = Dungeon::generate(&cfg).expect("gen");
    let idx = |c: IVec2| (c.y as usize) * d.width + c.x as usize;
    let mut seen = vec![false; d.width * d.height];
    let mut stack = vec![d.spawn];
    seen[idx(d.spawn)] = true;
    while let Some(c) = stack.pop() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = IVec2::new(c.x + dx, c.y + dy);
            if n.x >= 0
                && n.y >= 0
                && (n.x as usize) < d.width
                && (n.y as usize) < d.height
                && d.is_floor(n)
                && !seen[idx(n)]
            {
                seen[idx(n)] = true;
                stack.push(n);
            }
        }
    }
    let unreached = (0..d.width * d.height)
        .filter(|&i| d.walkable[i] && !seen[i])
        .count();
    assert_eq!(
        unreached, 0,
        "notching orphaned {unreached} floor cells from the spawn"
    );
}

#[test]
fn generate_is_deterministic_for_a_config() {
    let config = test_config();
    let a = Dungeon::generate(&config).expect("gen a");
    let b = Dungeon::generate(&config).expect("gen b");
    assert_eq!(
        a.walkable, b.walkable,
        "same (config, seed) → same walkable mask"
    );
    assert_eq!(a.spawn, b.spawn);
    assert_eq!(a.regions.len(), b.regions.len());
    for (ra, rb) in a.regions.iter().zip(&b.regions) {
        assert_eq!(ra.rect, rb.rect);
        assert_eq!(ra.props.tags, rb.props.tags);
    }
}

#[test]
fn every_region_carries_a_type_tag() {
    let config = test_config();
    let type_tags: Vec<&str> = config.room_types.iter().map(|t| t.tag.as_str()).collect();
    let d = Dungeon::generate(&config).expect("gen");
    for r in &d.regions {
        assert!(
            r.props.has("room"),
            "region {} missing base 'room' tag",
            r.id
        );
        assert!(
            r.props.tags.iter().any(|t| type_tags.contains(&t.as_str())),
            "region {} has no room-type tag: {:?}",
            r.id,
            r.props.tags
        );
    }
}

#[test]
fn room_dims_fit_block_with_margin() {
    let config = test_config();
    let max_side = (config.block - 2) as i32;
    let d = Dungeon::generate(&config).expect("gen");
    for r in &d.regions {
        let (w, h) = (r.rect.width(), r.rect.height());
        assert!(
            w >= ROOM_FLOOR as i32 && w <= max_side,
            "room width {w} out of range"
        );
        assert!(
            h >= ROOM_FLOOR as i32 && h <= max_side,
            "room height {h} out of range"
        );
    }
}

#[test]
fn zero_room_config_returns_err_not_panic() {
    // A 1×1 coarse grid: the sole cell borders the void on all four edges, so the boundary rule
    // (`wfc::boundary_initial`) forbids every Link and it collapses to rock → zero rooms → a loud
    // Err, never a panic. Exercises the `Result` path that replaced the old `.expect(...)`.
    let mut config = test_config();
    config.coarse_w = 1;
    config.coarse_h = 1;
    assert!(Dungeon::generate(&config).is_err());
}

#[test]
fn config_validation_rejects_bad_values() {
    // parse_config fails loud at the door on invalid input (one path, no silent default).
    assert!(parse_config("not ron").is_err());
    let bad_liminality = r#"(coarse_w:6,coarse_h:6,block:32,corridor_width:2,seed:1,max_attempts:20,
        liminality:2.0,wfc_weights:(rock:6.0,dead_end:1.2,corridor:2.5,corner:2.5,tee:1.2,cross:0.6),
        room_types:[(tag:"a",area_min:3.0,area_max:6.0,aspect_min:1.0,aspect_max:1.6,weight:1.0)])"#;
    assert!(
        parse_config(bad_liminality).is_err(),
        "liminality > 1 must be rejected"
    );
    let empty_types = r#"(coarse_w:6,coarse_h:6,block:32,corridor_width:2,seed:1,max_attempts:20,
        liminality:1.0,wfc_weights:(rock:6.0,dead_end:1.2,corridor:2.5,corner:2.5,tee:1.2,cross:0.6),
        room_types:[])"#;
    assert!(
        parse_config(empty_types).is_err(),
        "empty room_types must be rejected"
    );
}

#[test]
fn doorway_width_is_proportional_and_clamped() {
    // Proportional to the corridor's carved width, always ≥1, never wider than the corridor.
    // At ratio 0.5 a 2-wide corridor still necks to 1, but 3/4/5-wide corridors open 2/2/3 lanes —
    // so passage width varies instead of every doorway being one body wide (player report 2026-07-19).
    assert_eq!(doorway_width(2, 0.5), 1);
    assert_eq!(doorway_width(3, 0.5), 2); // round(1.5) = 2
    assert_eq!(doorway_width(4, 0.5), 2);
    assert_eq!(doorway_width(5, 0.5), 3); // round(2.5) = 3
    // ratio 1.0 opens the full corridor; a tiny ratio still keeps at least one lane; never > corridor.
    assert_eq!(doorway_width(5, 1.0), 5);
    assert_eq!(doorway_width(4, 0.05), 1);
    assert_eq!(doorway_width(1, 1.0), 1);
}

#[test]
fn validate_config_rejects_bad_doorway_ratio() {
    let mut cfg = test_config();
    cfg.doorway_ratio = 0.0;
    assert!(validate_config(&cfg).is_err(), "0 doorway_ratio must be rejected (empty (0,1])");
    cfg.doorway_ratio = 1.5;
    assert!(validate_config(&cfg).is_err(), ">1 doorway_ratio must be rejected");
    cfg.doorway_ratio = f32::NAN;
    assert!(validate_config(&cfg).is_err(), "NaN doorway_ratio must be rejected");
    cfg.doorway_ratio = 0.5;
    assert!(validate_config(&cfg).is_ok(), "an in-range doorway_ratio is accepted");
}

#[test]
fn doorways_open_a_proportional_band_of_floor_lanes() {
    // Fix the corridor width so the doorway width is deterministic: every corridor is exactly 3 wide,
    // so at ratio 0.5 every doorway necks to round(1.5) = 2 lanes. Assert each opening (a) records
    // width 2, and (b) has BOTH of its lanes (stacked perpendicular to `dir` from the mouth) as
    // walkable floor — i.e. the doorway is genuinely 2 wide, not necked back to a single body-width.
    let mut cfg = test_config();
    cfg.corridor_width = 3;
    cfg.corridor_width_max = None; // every corridor exactly 3 wide
    cfg.doorway_ratio = 0.5;
    let d = Dungeon::generate(&cfg).expect("gen");
    let mut checked = 0usize;
    for r in &d.regions {
        for op in &r.openings {
            assert_eq!(op.width, 2, "a 3-wide corridor at ratio 0.5 opens a 2-lane doorway");
            for lane in 0..op.width as i32 {
                let cell = match op.dir {
                    E | W => IVec2::new(op.cell[0], op.cell[1] + lane),
                    N | S => IVec2::new(op.cell[0] + lane, op.cell[1]),
                    _ => IVec2::new(op.cell[0], op.cell[1]),
                };
                assert!(d.is_floor(cell), "open doorway lane {cell:?} must be walkable floor");
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "a connected dungeon must have at least one doorway to check");
}

#[test]
fn config_validation_rejects_bad_wfc_weights() {
    // The grid WFC weights feed `collapse_one`; a NaN, a negative, a zero sum, or an all-rock
    // (floorless) distribution must fail at the door rather than silently degenerate the dungeon.
    let with_wfc = |wfc: &str| {
        format!(
            r#"(coarse_w:6,coarse_h:6,block:32,corridor_width:2,seed:1,max_attempts:20,
            liminality:1.0,wfc_weights:{wfc},
            room_types:[(tag:"a",area_min:3.0,area_max:6.0,aspect_min:1.0,aspect_max:1.6,weight:1.0)])"#
        )
    };
    assert!(
        parse_config(&with_wfc(
            "(rock:6.0,dead_end:1.2,corridor:NaN,corner:2.5,tee:1.2,cross:0.6)"
        ))
        .is_err(),
        "a NaN wfc_weight must be rejected"
    );
    assert!(
        parse_config(&with_wfc(
            "(rock:6.0,dead_end:1.2,corridor:-2.5,corner:2.5,tee:1.2,cross:0.6)"
        ))
        .is_err(),
        "a negative wfc_weight must be rejected"
    );
    assert!(
        parse_config(&with_wfc(
            "(rock:0.0,dead_end:0.0,corridor:0.0,corner:0.0,tee:0.0,cross:0.0)"
        ))
        .is_err(),
        "a zero-sum wfc_weights must be rejected"
    );
    assert!(
        parse_config(&with_wfc(
            "(rock:6.0,dead_end:0.0,corridor:0.0,corner:0.0,tee:0.0,cross:0.0)"
        ))
        .is_err(),
        "an all-rock (floorless) wfc_weights must be rejected"
    );
    assert!(
        parse_config(&with_wfc(
            "(rock:6.0,dead_end:1.2,corridor:2.5,corner:2.5,tee:1.2,cross:0.6)"
        ))
        .is_ok(),
        "a valid wfc_weights distribution must parse"
    );
}

#[test]
fn liminality_1_centers_rooms() {
    // At liminality 1.0 (t=0) jitter_origin is a no-op: every room stays block-centred (the shipped
    // grid). ox = cx*block + (block-rw)/2, so ox % block == (block-rw)/2.
    let mut config = test_config();
    config.liminality = 1.0;
    let block = config.block;
    let d = Dungeon::generate(&config).expect("gen");
    for r in &d.regions {
        let w = r.rect.width() as usize;
        let h = r.rect.height() as usize;
        assert_eq!(
            r.rect.min[0] as usize % block,
            (block - w) / 2,
            "room not x-centred"
        );
        assert_eq!(
            r.rect.min[1] as usize % block,
            (block - h) / 2,
            "room not y-centred"
        );
    }
}

#[test]
fn liminality_0_still_generates_connected_rooms() {
    // At liminality 0.0 (max jitter) generation still succeeds with rooms + floor — the jitter is
    // bounded to keep each block centre interior, so corridors still connect. And at least one room
    // slides off its centred position, so the dial demonstrably did something.
    let mut config = test_config();
    config.liminality = 0.0;
    let block = config.block;
    let d = Dungeon::generate(&config).expect("gen at liminality 0");
    assert!(!d.regions.is_empty());
    assert!(d.walkable.iter().any(|&w| w), "must have floor");
    let any_offset = d.regions.iter().any(|r| {
        let w = r.rect.width() as usize;
        let h = r.rect.height() as usize;
        r.rect.min[0] as usize % block != (block - w) / 2
            || r.rect.min[1] as usize % block != (block - h) / 2
    });
    assert!(
        any_offset,
        "liminality 0 should slide at least one room off-centre"
    );
}

#[test]
fn liminality_0_rooms_never_overlap() {
    // Expansion-to-touch grows rooms toward their links, but each stays within its own block, so no
    // two rooms ever overlap — a safety net on the extension math at maximum growth.
    let mut config = test_config();
    config.liminality = 0.0;
    let d = Dungeon::generate(&config).expect("gen");
    let overlaps = |a: &Rect2, b: &Rect2| {
        a.min[0] < b.max[0] && b.min[0] < a.max[0] && a.min[1] < b.max[1] && b.min[1] < a.max[1]
    };
    for (i, a) in d.regions.iter().enumerate() {
        for b in &d.regions[i + 1..] {
            assert!(
                !overlaps(&a.rect, &b.rect),
                "regions {} and {} overlap",
                a.id,
                b.id
            );
        }
    }
}

// ---- Phase 3 Step 5: Graph topology (Poisson + Delaunay + collapse_graph) integration ----------

/// A `Topology::Graph` config over a 96×96 level (~40 Poisson sites at spacing 14).
fn graph_test_config() -> DungeonConfig {
    let mut c = test_config();
    c.coarse_w = 6;
    c.coarse_h = 6;
    c.block = 16;
    c.topology = Topology::Graph {
        site_spacing: 14.0,
        link_weights: [0.05, 1.2, 2.5, 1.2, 0.6, 0.6],
    };
    c
}

/// Flood-fill `d.walkable` (4-connected) from `d.spawn`, returning the reached-cell mask.
fn reachable_from_spawn(d: &Dungeon) -> Vec<bool> {
    let (w, h) = (d.width, d.height);
    let mut seen = vec![false; w * h];
    let start = d.spawn.y as usize * w + d.spawn.x as usize;
    assert!(d.walkable[start], "spawn must be on a walkable cell");
    seen[start] = true;
    let mut stack = vec![start];
    while let Some(i) = stack.pop() {
        let (x, y) = ((i % w) as i32, (i / w) as i32);
        for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                let ni = ny as usize * w + nx as usize;
                if d.walkable[ni] && !seen[ni] {
                    seen[ni] = true;
                    stack.push(ni);
                }
            }
        }
    }
    seen
}

/// Assert every region has at least one interior floor cell reachable from spawn — true *geometric*
/// connectivity over the carved `walkable` mask, not just the logical `region.adjacency` graph (which
/// is connected by construction and so cannot catch a room whose corridor misses its rect).
fn assert_all_regions_reachable(d: &Dungeon) {
    let seen = reachable_from_spawn(d);
    let w = d.width;
    for r in &d.regions {
        let reached = (r.rect.min[1]..r.rect.max[1])
            .any(|y| (r.rect.min[0]..r.rect.max[0]).any(|x| seen[y as usize * w + x as usize]));
        assert!(reached, "region {} has no floor reachable from spawn", r.id);
    }
}

fn assert_no_overlap(d: &Dungeon) {
    let overlaps = |a: &Rect2, b: &Rect2| {
        a.min[0] < b.max[0] && b.min[0] < a.max[0] && a.min[1] < b.max[1] && b.min[1] < a.max[1]
    };
    for (i, a) in d.regions.iter().enumerate() {
        for b in &d.regions[i + 1..] {
            assert!(
                !overlaps(&a.rect, &b.rect),
                "graph regions {} and {} overlap",
                a.id,
                b.id
            );
        }
    }
}

#[test]
fn graph_topology_generates_connected_non_overlapping_rooms() {
    let config = graph_test_config();
    let d = Dungeon::generate(&config).expect("graph topology must generate");
    assert!(!d.regions.is_empty(), "graph must produce rooms");
    assert!(d.walkable.iter().any(|&w| w), "graph must have floor");
    assert_no_overlap(&d);
    assert_all_regions_reachable(&d);
}

#[test]
fn graph_topology_generates_at_liminality_0() {
    // Exercise the graph carve's RNG-drawing geometry (room jitter + expansion-to-touch, t = 1) —
    // the grid golden covers liminality 0.0, but the other graph tests run only at t = 0.
    let mut config = graph_test_config();
    config.liminality = 0.0;
    let d = Dungeon::generate(&config).expect("graph must generate at liminality 0");
    assert!(!d.regions.is_empty() && d.walkable.iter().any(|&x| x));
    assert_no_overlap(&d);
    assert_all_regions_reachable(&d);
}

// #4 regression + a target invariant for the deferred #5 work. Openings now land on the correct wall
// for the L-route (the #4 `derive_opening` fix, incl. the L-corner-inside-room case) and every room is
// reachable, but this stricter check — every doorway faces a real corridor mouth — still trips on the
// known Graph limitation #5: a Delaunay node can have >4 neighbours (forced at degree 5), so two
// corridors can share a wall and one's necking rocks the other's lane-0 mouth. That is doorway
// cosmetics, not connectivity (see graph_topology_generates_connected...). Un-ignore once corridors
// fan out along the wall / necking is coordinated across same-wall openings.
#[test]
#[ignore = "known Graph limitation #5: multiple corridors per wall can rock a lane-0 doorway mouth"]
fn graph_openings_sit_on_real_corridor_mouths() {
    // Every recorded Opening must lie on its region's perimeter per its `dir`, AND the cell one step
    // OUT of the room must be walkable — i.e. the door faces the corridor the L-route actually carved.
    let config = graph_test_config();
    let d = Dungeon::generate(&config).expect("gen");
    let (w, h) = (d.width as i32, d.height as i32);
    for r in &d.regions {
        for o in &r.openings {
            let [cx, cy] = o.cell;
            match o.dir {
                N => assert_eq!(
                    cy, r.rect.min[1],
                    "N opening off the N wall of region {}",
                    r.id
                ),
                S => assert_eq!(
                    cy,
                    r.rect.max[1] - 1,
                    "S opening off the S wall of region {}",
                    r.id
                ),
                E => assert_eq!(
                    cx,
                    r.rect.max[0] - 1,
                    "E opening off the E wall of region {}",
                    r.id
                ),
                W => assert_eq!(
                    cx, r.rect.min[0],
                    "W opening off the W wall of region {}",
                    r.id
                ),
                _ => unreachable!(),
            }
            let (ox, oy) = match o.dir {
                N => (cx, cy - 1),
                S => (cx, cy + 1),
                E => (cx + 1, cy),
                W => (cx - 1, cy),
                _ => unreachable!(),
            };
            assert!(
                ox >= 0 && oy >= 0 && ox < w && oy < h,
                "opening mouth off-grid on region {}",
                r.id
            );
            assert!(
                d.walkable[oy as usize * d.width + ox as usize],
                "region {} opening (dir {}) faces a wall, not a corridor",
                r.id,
                o.dir
            );
        }
    }
}

#[test]
fn graph_topology_is_deterministic() {
    let config = graph_test_config();
    let a = Dungeon::generate(&config).expect("gen a");
    let b = Dungeon::generate(&config).expect("gen b");
    assert_eq!(
        a.walkable, b.walkable,
        "same graph config + seed → same walkable mask"
    );
    assert_eq!(a.spawn, b.spawn);
    assert_eq!(a.regions.len(), b.regions.len());
}

#[test]
fn topology_defaults_to_grid_when_absent() {
    // The shipped config.ron `dungeon:` slice has no `topology` field → serde default → Grid.
    let config = crate::config::load_game_config().expect("valid").dungeon;
    assert!(matches!(config.topology, Topology::Grid), "absent topology must default to Grid");
}

#[test]
fn graph_config_validation() {
    let base = r#"(coarse_w:6,coarse_h:6,block:16,corridor_width:2,seed:1,max_attempts:20,
        liminality:1.0,wfc_weights:(rock:6.0,dead_end:1.2,corridor:2.5,corner:2.5,tee:1.2,cross:0.6),
        room_types:[(tag:"a",area_min:3.0,area_max:6.0,aspect_min:1.0,aspect_max:1.6,weight:1.0)],"#;
    // NB: serde encodes `[f64; 6]` as a *tuple*, so RON writes `link_weights` with `(...)`, not `[...]`.
    let small = format!(
        "{base}topology:Graph(site_spacing:3.0,link_weights:(0.05,1.2,2.5,1.2,0.6,0.6)))"
    );
    assert!(
        parse_config(&small).is_err(),
        "site_spacing below the floor must be rejected"
    );
    let zero = format!(
        "{base}topology:Graph(site_spacing:14.0,link_weights:(0.0,0.0,0.0,0.0,0.0,0.0)))"
    );
    assert!(
        parse_config(&zero).is_err(),
        "zero-sum link_weights must be rejected"
    );
    let ok = format!(
        "{base}topology:Graph(site_spacing:14.0,link_weights:(0.05,1.2,2.5,1.2,0.6,0.6)))"
    );
    let cfg = parse_config(&ok).expect("valid graph config must parse");
    assert!(
        Dungeon::generate(&cfg).is_ok(),
        "valid graph config must generate"
    );
}

// ---- Dungeon carve golden: locks the full Grid carve output so unintended drift in geometry, RNG
// draw order, or region-link order flips the hash. FNV-1a over the FULL Dungeon output (dims, walkable
// mask, spawn, and every region's rect/tags/adjacency/openings). Uses `test_config()` (self-contained)
// rather than the shipped RON so the gate is stable even while `assets/config/config.ron` is edited.
// Order: liminality 1.0 for seeds [1,2,3], then liminality 0.0 for seeds [1,2,3] (1.0 draws zero
// jitter RNG, so 0.0 must be covered too to exercise the `jitter_origin` draw path).
// Re-pinned for the layout-diversity work: per-corridor width variation (`corridor_width_max`) and
// type-aware expansion (`RoomType::expands`) deliberately change the carve — a legitimate worldgen
// change with sign-off, not accidental drift.
// Re-pinned 2026-07-19: proportional doorway widths (`doorway_ratio`, default 0.5) legitimately
// widen every corridor mouth beyond the old 1-tile neck, so the Grid carve changed with sign-off.
const GOLDEN_DUNGEON: [u64; 6] = [
    15713791351976089880,
    9641188687789941418,
    11115380735315394124,
    6071063408633908684,
    14844288061605299761,
    11008265804405502608,
];

/// FNV-1a accumulator — deterministic across runs (unlike `DefaultHasher`'s per-process seed).
struct Fnv(u64);
impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn push(&mut self, v: u64) {
        self.0 ^= v;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }
    fn push_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.push(b as u64);
        }
        self.push(0xFF); // field separator
    }
}

fn fingerprint(d: &Dungeon) -> u64 {
    let mut f = Fnv::new();
    f.push(d.width as u64);
    f.push(d.height as u64);
    for &w in &d.walkable {
        f.push(w as u64);
    }
    f.push(d.spawn.x as u64);
    f.push(d.spawn.y as u64);
    f.push(d.regions.len() as u64);
    for r in &d.regions {
        f.push(r.id as u64);
        f.push(r.rect.min[0] as u64);
        f.push(r.rect.min[1] as u64);
        f.push(r.rect.max[0] as u64);
        f.push(r.rect.max[1] as u64);
        for t in &r.props.tags {
            f.push_str(t);
        }
        f.push(0xF0F0);
        for &a in &r.adjacency {
            f.push(a as u64);
        }
        f.push(0x0F0F);
        for o in &r.openings {
            f.push(o.dir as u64);
            f.push(o.cell[0] as u64);
            f.push(o.cell[1] as u64);
        }
    }
    f.0
}

fn golden_fingerprints() -> Vec<u64> {
    let base = test_config();
    let mut fps = Vec::new();
    for lim in [1.0f32, 0.0] {
        for seed in [1u64, 2, 3] {
            let mut cfg = base.clone();
            cfg.seed = seed;
            cfg.liminality = lim;
            let d = Dungeon::generate(&cfg).expect("golden config must generate");
            fps.push(fingerprint(&d));
        }
    }
    fps
}

#[test]
fn golden_dungeon_snapshot_is_stable() {
    // Byte-identical gate for the Grid carve. If this fails after a change meant to be behaviour-
    // neutral, the carve drifted; if the change was intentional (a worldgen tweak), re-pin from the
    // printed value with sign-off.
    let fps = golden_fingerprints();
    println!("GOLDEN_DUNGEON = {fps:?}");
    assert_eq!(
        fps.as_slice(),
        &GOLDEN_DUNGEON,
        "dungeon carve output changed"
    );
}

/// Footprint-aware containment (README ISSUES 1 & 2): a piece is legal only when its whole body
/// lies on floor, never when it overhangs a wall or a notched-out corner — the discrete
/// `C_free` non-penetration test (Merrell et al. 2011).
#[test]
fn footprint_on_floor_rejects_wall_overhang() {
    // A 3×3 floor room (cells (1,1)..=(3,3)) walled in by rock in a 5×5 grid.
    let mut mask = vec![false; 5 * 5];
    for y in 1..4 {
        for x in 1..4 {
            mask[y * 5 + x] = true;
        }
    }
    let d = Dungeon::from_walkable(5, 5, mask);

    // A small piece dead-centre in the interior cell (2,2) is fully clear.
    assert!(d.footprint_on_floor(Vec3::new(2.0, 0.0, 2.0), Vec2::new(0.1, 0.1), 0.0));

    // A large piece at a corner cell (1,1) overhangs the N and W walls → rejected. Its old
    // center-only `is_floor` check would have wrongly accepted it.
    assert!(d.is_floor(IVec2::new(1, 1)));
    assert!(!d.footprint_on_floor(Vec3::new(1.0, 0.0, 1.0), Vec2::new(0.4, 0.4), 0.0));

    // A piece centred on a rock cell (outside the room) is rejected outright.
    assert!(!d.footprint_on_floor(Vec3::new(0.0, 0.0, 0.0), Vec2::new(0.1, 0.1), 0.0));

    // Quarter-turn: a long-thin piece at edge cell (2,1) (walled on N) clears at yaw 0 (long axis
    // runs along X, away from the wall) but overhangs once rotated 90° (long axis into the N wall).
    let half = Vec2::new(0.4, 0.1); // 0.8 (w) × 0.2 (d)
    assert!(d.footprint_on_floor(Vec3::new(2.0, 0.0, 1.0), half, 0.0));
    assert!(!d.footprint_on_floor(Vec3::new(2.0, 0.0, 1.0), half, std::f32::consts::FRAC_PI_2));
}

/// Regression for the player-reported "picket fence" wall bug (debug capture 2026-07-25,
/// `region_2026-07-25_12-27-00-608.png`): a unit standing off a 1-wide corridor's own row
/// loses strict [`Dungeon::line_of_sight`] to corridor cells past the doorway, because the
/// diagonal step's "far" orthogonal neighbour is the corridor's own bounding wall — not a true
/// diagonal pinch. [`Dungeon::line_of_sight_reveal`] must see every one of those cells so fog
/// reveal doesn't leave alternating wall segments stuck `Unseen`; strict `line_of_sight` must
/// keep blocking at least one of them (so pathfinding/laser retain the no-corner-cutting rule).
#[test]
fn line_of_sight_reveal_sees_down_a_corridor_that_strict_los_partly_blocks() {
    let w = 24usize;
    let h = 8usize;
    let mut mask = vec![false; w * h];
    let mut set = |x: i32, y: i32| mask[(y as usize) * w + (x as usize)] = true;
    for x in 0..4 {
        for y in 4..7 {
            set(x, y);
        }
    }
    for x in 4..20 {
        set(x, 5);
    }
    let d = Dungeon::from_walkable(w, h, mask);
    let uc = IVec2::new(1, 4);

    let strict: String = (4..20)
        .map(|x| if d.line_of_sight(uc, IVec2::new(x, 5)) { 'T' } else { 'F' })
        .collect();
    let lenient: String = (4..20)
        .map(|x| if d.line_of_sight_reveal(uc, IVec2::new(x, 5)) { 'T' } else { 'F' })
        .collect();
    println!("uc={uc:?} corridor row y=5, x=4..20: strict={strict} lenient={lenient}");

    // Cells (5,5) and (6,5): strict LOS blocks them purely on the diagonal-corner rule (the
    // "far" neighbour is this corridor's own bounding wall, not a true diagonal pinch) — the
    // exact picket-fence signature. `line_of_sight_reveal` must see both.
    for x in [5, 6] {
        let c = IVec2::new(x, 5);
        assert!(!d.line_of_sight(uc, c), "fixture must reproduce strict-LOS blocking at {c:?}");
        assert!(
            d.line_of_sight_reveal(uc, c),
            "line_of_sight_reveal must see {c:?} — strict-only diagonal-corner block, not a \
             genuine occlusion (got lenient={lenient})"
        );
    }
    // Deep corridor cells at this shallow an angle can be genuinely occluded — the sightline's
    // pure-x steps pass through the room/corridor divider (rock at row 4, x >= 4) before the
    // line's single y-increment ever happens. That's real wall occlusion, not the bug; the
    // lenient variant must not paper over it.
    assert!(
        lenient.contains('F'),
        "expected some genuinely occluded deep cell under the lenient variant (strict={strict} \
         lenient={lenient}) — if this now fails, line_of_sight_reveal may have gone too far \
         and stopped blocking real occlusion"
    );
}

/// The corridor fixture above never reaches the lenient rule's own *blocking* branch — measured,
/// it contains no diagonal step whose two orthogonal neighbours are both solid, so
/// `blocked = !n1 && !n2` is never evaluated there and a change that deleted the lenient block
/// entirely would still pass it. These are the two discriminating cases, one per side of the
/// rule, on minimal hand-built grids.
///
/// The distinction is a deliberate design choice, not a computational detail: a diagonal
/// corner-peek is an observable visibility rule, so both sides of it get pinned.
#[test]
fn lenient_los_blocks_a_closed_diagonal_pinch_but_not_a_single_corner() {
    // Two rooms meeting corner-to-corner: BOTH orthogonal neighbours of the diagonal step are
    // rock, a genuine closed pinch. Neither variant may see through it.
    let mut mask = vec![false; 4 * 4];
    mask[0 * 4] = true; // (0,0)
    mask[1 * 4 + 1] = true; // (1,1)
    let pinch = Dungeon::from_walkable(4, 4, mask);
    let (a, b) = (IVec2::new(0, 0), IVec2::new(1, 1));
    assert!(!pinch.line_of_sight(a, b), "strict LOS must block a closed diagonal pinch");
    assert!(
        !pinch.line_of_sight_reveal(a, b),
        "lenient LOS must STILL block a closed diagonal pinch — two walls meeting corner to \
         corner is real occlusion, and this is the branch the corridor fixture never reaches"
    );

    // One neighbour open: the sightline runs alongside a wall rather than through a pinch. This
    // is the picket-fence case in miniature — strict blocks, reveal must not.
    let mut mask = vec![false; 4 * 4];
    mask[0 * 4] = true; // (0,0)
    mask[1] = true; // (1,0) — the open orthogonal neighbour
    mask[1 * 4 + 1] = true; // (1,1)
    let corner = Dungeon::from_walkable(4, 4, mask);
    assert!(!corner.line_of_sight(a, b), "strict LOS blocks on any solid diagonal neighbour");
    assert!(
        corner.line_of_sight_reveal(a, b),
        "lenient LOS must see past a corner with one open orthogonal neighbour"
    );
}

/// Symmetry is a *property*, so pin it as one rather than as a sentence in a doc comment: for
/// every pair of cells, `line_of_sight(p, q)` must equal `line_of_sight(q, p)`, under both corner
/// rules. This is what the doc on [`Dungeon::line_of_sight`] used to merely assert while the walk
/// did the opposite — Bresenham seeds its error term from the start cell, so the reverse walk
/// crosses different cells on a diagonal tie and the two directions disagree wherever one of
/// those cells is rock. Run against the pre-canonicalisation code this fails on the first map.
#[test]
fn line_of_sight_is_symmetric_over_random_maps() {
    // Test-local xorshift32: pure, seeded, no ECS query and no shared RNG behind it.
    fn xorshift(s: &mut u32) -> u32 {
        *s ^= *s << 13;
        *s ^= *s >> 17;
        *s ^= *s << 5;
        *s
    }
    const N: i32 = 16;
    let mut s: u32 = 0x0A11_CE01;
    for map in 0..200 {
        // ~60% floor: dense enough to have long sightlines, holed enough to have real corners.
        let mask: Vec<bool> = (0..N * N).map(|_| xorshift(&mut s) % 100 < 60).collect();
        let d = Dungeon::from_walkable(N as usize, N as usize, mask);
        let cell = |s: &mut u32| {
            IVec2::new((xorshift(s) % N as u32) as i32, (xorshift(s) % N as u32) as i32)
        };
        for _ in 0..200 {
            let (a, b) = (cell(&mut s), cell(&mut s));
            assert_eq!(
                d.line_of_sight(a, b),
                d.line_of_sight(b, a),
                "strict LOS asymmetric on map {map}: {a:?} <-> {b:?}"
            );
            assert_eq!(
                d.line_of_sight_reveal(a, b),
                d.line_of_sight_reveal(b, a),
                "lenient LOS asymmetric on map {map}: {a:?} <-> {b:?}"
            );
        }
    }
}
