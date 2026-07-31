//! Coarse layout: both topologies (grid lattice and Poisson/Delaunay graph) produce a
//! `CoarseLayout` and hand it to the one fine carver `expand_to_fine`, so furnish/nav/fog see a single
//! shape regardless of which front-end ran.
//! Split out of the former single-file `dungeon.rs` (3,447 lines) — a **pure move**, no logic
//! changed, so the replay goldens are untouched (FVS-N-1). `use super::*` at the top of each submodule
//! inherits the parent's imports, which is what keeps the move mechanical and reviewable: the diff is
//! whole items relocated, not hundreds of rewritten `use` lines.

use super::*;

// ── Topology-agnostic coarse layer ───────────────────────────────────────────────────────────────
// Both dungeon topologies — the fixed grid lattice and (Phase 3) the Poisson/Delaunay graph — produce
// a `CoarseLayout` and hand it to the single fine carver `expand_to_fine`, so furnish/nav/fog
// never depend on which topology ran. The grid front-end (`grid_layout`) is a byte-identical restatement
// of the old carve's coarse phase (Step-0 golden gate); the graph front-end is added later.

/// One room slot: its fine-grid centre and the block-like extent that bounds its room sizing, jitter,
/// and expansion-to-touch. For the grid, `bounds` is the exact block rect and `center` its centre.
pub(crate) struct Site {
    center: IVec2,
    bounds: Rect2,
}

/// The coarse room graph handed to `expand_to_fine`, independent of how it was produced. `sites` are in
/// carve order (the grid emits them row-major over kept slots, so RNG draws and region ids stay in
/// lockstep with the pre-refactor loop). `adjacency` is the undirected set of corridor links between kept
/// sites (already trimmed to one connected component). `spawn_site` is chosen by the front-end (each
/// topology owns its spawn rule) so `expand_to_fine` never re-derives it.
pub(crate) struct CoarseLayout {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) sites: Vec<Site>,
    pub(crate) adjacency: Vec<(usize, usize)>,
    pub(crate) spawn_site: usize,
}

/// The wall each endpoint of a corridor actually pierces, matching `carve_corridor`'s L-route: the
/// horizontal leg runs from `a`, the vertical leg into `b`. So `a` exits horizontally (E/W) unless the
/// edge is purely vertical, and `b` enters vertically (N/S) unless the edge is purely horizontal.
/// Returns `(a_dir, b_dir)`. For axis-aligned edges — always, for grid block centres offset by exactly
/// `(±block, 0)` or `(0, ±block)` — both reduce to the straight-line cardinal, so the Grid path stays
/// byte-identical; for a diagonal graph edge each end gets the wall its leg crosses, not a dominant-axis
/// guess (which would land one endpoint's door on a wall the corridor never touches).
fn corridor_exit_dirs(a: IVec2, b: IVec2) -> (usize, usize) {
    let a_dir = if a.x == b.x {
        if b.y > a.y {
            S
        } else {
            N
        }
    } else if b.x > a.x {
        E
    } else {
        W
    };
    let b_dir = if a.y == b.y {
        if a.x > b.x {
            E
        } else {
            W
        }
    } else if a.y > b.y {
        S
    } else {
        N
    };
    (a_dir, b_dir)
}

/// Where a corridor pierces one endpoint's room wall, from the carved L geometry (see `carve_corridor`).
/// `sc` is this room's site centre, `nc` the neighbour's, `rect` this room's rect, `is_first` whether this
/// site is the edge's FIRST endpoint (horizontal leg leaves it) or SECOND (vertical leg enters it).
/// Returns `(dir, cell)`. The first endpoint always exits through its own centre row/col. The second is
/// entered vertically (N/S) unless the L-corner `(sc.x, nc.y)` lands inside this room's y-range — then the
/// corridor actually enters via the horizontal leg through the E/W wall at row `nc.y`. This handles the
/// L-corner-inside-room case a dominant-axis guess gets wrong, and is byte-identical to the old
/// block-centre formula for axis-aligned (grid) edges.
fn derive_opening(sc: IVec2, nc: IVec2, rect: Rect2, is_first: bool) -> (usize, [i32; 2]) {
    if is_first {
        if sc.x == nc.x {
            if nc.y > sc.y {
                (S, [sc.x, rect.max[1] - 1])
            } else {
                (N, [sc.x, rect.min[1]])
            }
        } else if nc.x > sc.x {
            (E, [rect.max[0] - 1, sc.y])
        } else {
            (W, [rect.min[0], sc.y])
        }
    } else if nc.y == sc.y {
        // Pure-horizontal edge: the second endpoint is entered horizontally at its own centre row.
        if nc.x < sc.x {
            (W, [rect.min[0], sc.y])
        } else {
            (E, [rect.max[0] - 1, sc.y])
        }
    } else if nc.y < rect.min[1] {
        (N, [sc.x, rect.min[1]])
    } else if nc.y >= rect.max[1] {
        (S, [sc.x, rect.max[1] - 1])
    } else if nc.x < sc.x {
        // L-corner inside this room's y-range → entered via the horizontal leg through the W/E wall.
        (W, [rect.min[0], nc.y])
    } else {
        (E, [rect.max[0] - 1, nc.y])
    }
}

/// Build the Grid topology's `CoarseLayout` from a collapsed coarse WFC grid + its kept-slot mask. Sites
/// are the kept block centres (row-major, preserving carve order); each site's `bounds` is its exact
/// block rect; adjacency is every kept∧kept∧`open` edge (E and S per slot, each counted once, oriented
/// so the neighbour's W/N view is reconstructed from the socket-rule symmetry in `expand_to_fine`);
/// `spawn_site` is the kept slot nearest the coarse centre — the exact pre-refactor spawn rule.
pub(crate) fn grid_layout(
    coarse: &wfc::WfcResult,
    kept: &[bool],
    config: &DungeonConfig,
) -> Result<CoarseLayout, String> {
    let (cw, ch, block) = (config.coarse_w, config.coarse_h, config.block);
    let mut slot_site: Vec<Option<usize>> = vec![None; cw * ch];
    let mut sites: Vec<Site> = Vec::new();
    for cy in 0..ch {
        for cx in 0..cw {
            if !kept[cy * cw + cx] {
                continue;
            }
            slot_site[cy * cw + cx] = Some(sites.len());
            sites.push(Site {
                center: IVec2::new(
                    (cx * block + block / 2) as i32,
                    (cy * block + block / 2) as i32,
                ),
                bounds: Rect2 {
                    min: [(cx * block) as i32, (cy * block) as i32],
                    max: [((cx + 1) * block) as i32, ((cy + 1) * block) as i32],
                },
            });
        }
    }

    // E and S edges only (each undirected link counted once); the neighbour's W/N view is reconstructed
    // in `expand_to_fine`. `if let Some(b)` subsumes the old `kept[neighbour]` guard (a slot has a site
    // iff it is kept).
    let mut adjacency: Vec<(usize, usize)> = Vec::new();
    let coarse_open = |cx: usize, cy: usize, dir: usize| coarse.cells[cy * cw + cx].open[dir];
    for cy in 0..ch {
        for cx in 0..cw {
            let Some(a) = slot_site[cy * cw + cx] else {
                continue;
            };
            if cx + 1 < cw && coarse_open(cx, cy, E) {
                if let Some(b) = slot_site[cy * cw + cx + 1] {
                    adjacency.push((a, b));
                }
            }
            if cy + 1 < ch && coarse_open(cx, cy, S) {
                if let Some(b) = slot_site[(cy + 1) * cw + cx] {
                    adjacency.push((a, b));
                }
            }
        }
    }

    // Spawn at the kept slot nearest the coarse centre (total_cmp → no NaN/unwrap).
    let center = Vec2::new(cw as f32 / 2.0, ch as f32 / 2.0);
    let spawn_slot = (0..cw * ch)
        .filter(|&i| kept[i])
        .min_by(|&a, &b| {
            let pa = Vec2::new((a % cw) as f32, (a / cw) as f32);
            let pb = Vec2::new((b % cw) as f32, (b / cw) as f32);
            (pa - center)
                .length_squared()
                .total_cmp(&(pb - center).length_squared())
        })
        .ok_or_else(|| {
            "dungeon generation produced zero rooms (largest component empty)".to_string()
        })?;
    let spawn_site = slot_site[spawn_slot]
        .ok_or_else(|| "spawn slot was not a kept site (unreachable)".to_string())?;

    Ok(CoarseLayout {
        width: cw * block,
        height: ch * block,
        sites,
        adjacency,
        spawn_site,
    })
}

/// Carve the fine grid from a topology-agnostic `CoarseLayout`: one room per site (type-driven size,
/// liminality jitter + expansion-to-touch), one corridor per adjacency edge, then doorway openings +
/// necking. Shared by both topologies. Byte-identical to the pre-refactor grid carve when handed a
/// `grid_layout` (the Step-0 golden gate). The carve RNG (`config.seed ^ 0xC0FFEE`, created by the
/// caller) is drawn only in the room pass, in site order — so the Grid path's draw sequence is unchanged.
pub(crate) fn expand_to_fine(
    layout: &CoarseLayout,
    config: &DungeonConfig,
    rng: &mut impl DetRng,
) -> Result<(Vec<bool>, Vec<Region>, IVec2, Vec<u32>, Vec<Biome>), String> {
    let (width, height) = (layout.width, layout.height);
    let mut walkable = vec![false; width * height];
    let t = 1.0 - config.liminality; // 0 at Backrooms (liminality 1), 1 at realistic (liminality 0)

    // Per-site incidence, derived from adjacency once (no RNG). `incident[si]` = (sort_dir, neighbour,
    // is_first), sorted by (dir, neighbour) to drive the opening/necking pass in the same N,E,S,W order the
    // pre-refactor four-direction scan used. `is_first` marks the FIRST endpoint of the edge (carve_corridor
    // runs the horizontal leg from the first endpoint, the vertical leg into the second), which the opening
    // pass needs to derive each doorway from the real L geometry. Each undirected edge contributes its
    // cardinal to one endpoint and the opposite to the other (socket-rule symmetry).
    let mut incident: Vec<Vec<(usize, usize, bool)>> = vec![Vec::new(); layout.sites.len()];
    for &(a, b) in &layout.adjacency {
        let (da, db) = corridor_exit_dirs(layout.sites[a].center, layout.sites[b].center);
        incident[a].push((da, b, true));
        incident[b].push((db, a, false));
    }
    for inc in &mut incident {
        // SORT-OK: WFC graph edges from a seeded generator, not an ECS query.
        inc.sort_by_key(|&(dir, nb, _)| (dir, nb));
    }

    // Room pass — one room per site (the only RNG-consuming pass; draws stay in site order). Size + type
    // are drawn from the config's weighted room-type table (Merrell 2011); the room is block-centred at
    // liminality 1.0 and slides off-centre + grows toward its linked edges as liminality drops.
    let mut regions: Vec<Region> = Vec::new();
    // Which room slot claimed each cell, or [`NO_ROOM`]. The room pass is the only place this is
    // knowable — corridors are carved *through* room interiors afterwards, so "inside a room rect" stops
    // being the same question as "room floor" the moment the corridor pass runs. Recorded here for the
    // per-zone biome resolution (FVS-Q-8); see `resolve_biomes`.
    let mut room_of = vec![NO_ROOM; width * height];
    for (si, site) in layout.sites.iter().enumerate() {
        let (bmin, bmax) = (site.bounds.min, site.bounds.max);
        let (bw, bh) = ((bmax[0] - bmin[0]) as usize, (bmax[1] - bmin[1]) as usize);
        let max_side = bw.min(bh).saturating_sub(2); // keep a >=1-tile rock margin inside the block
        let (rw, rh, tag, expands) = pick_room(config, max_side, rng);
        let (bx, by) = (site.center.x, site.center.y);
        let cox = bmin[0] as usize + (bw - rw) / 2;
        let coy = bmin[1] as usize + (bh - rh) / 2;
        let ox = jitter_origin(cox, rw, bmin[0] as usize, bw, bx as usize, t, rng);
        let oy = jitter_origin(coy, rh, bmin[1] as usize, bh, by as usize, t, rng);

        // Expansion-to-touch (spacious types only, per `RoomType::expands`): a hall/large room grows toward
        // *all four* block edges — by fraction `t` of the gap — so it fills its slot and dominates as an
        // anchor space. Compact types don't grow, keeping their realistic drawn footprint so the size
        // hierarchy survives (tiny bathroom beside a sprawling hall). Growth is capped one cell short of the
        // block wall (a >=2-cell doorway gap always remains) and draws no RNG — liminality 1.0 (t=0) is a
        // no-op, and the block centre stays interior so every corridor still connects.
        let toward = |near: i32, cap: i32| near + ((cap - near) as f32 * t).round() as i32;
        let mut left = ox as i32;
        let mut right = (ox + rw) as i32;
        let mut top = oy as i32;
        let mut bot = (oy + rh) as i32;
        if expands {
            left = toward(left, bmin[0] + 1);
            right = toward(right, bmax[0] - 1);
            top = toward(top, bmin[1] + 1);
            bot = toward(bot, bmax[1] - 1);
        }
        let (ox, rw) = (left as usize, (right - left) as usize);
        let (oy, rh) = (top as usize, (bot - top) as usize);
        for y in oy..oy + rh {
            for x in ox..ox + rw {
                walkable[y * width + x] = true;
                // Sites own disjoint blocks and expansion is capped one cell short of the block wall, so
                // two rooms cannot claim the same cell. Were that ever to change, the last site in carve
                // order wins — deterministic, but it would want a rule rather than an accident.
                room_of[y * width + x] = si as u32;
            }
        }

        // Corner-notching (shape complexity): bite chunks out of the room's corners so it reads as an
        // L / T / plus (6–12 corners) instead of a plain box. Draws RNG in site order like the rest of the
        // room pass, so replays stay deterministic; `None` (no `notch` in the config) leaves rooms rectangular.
        if let Some(nc) = &config.notch {
            notch_room(
                &mut walkable,
                width,
                ox,
                oy,
                rw,
                rh,
                bx as usize,
                by as usize,
                nc,
                rng,
            );
        }

        regions.push(Region {
            id: si as u32,
            rect: Rect2 {
                min: [ox as i32, oy as i32],
                max: [(ox + rw) as i32, (oy + rh) as i32],
            },
            openings: Vec::new(),
            adjacency: Vec::new(),
            props: PropertyBag {
                tags: vec!["room".to_string(), tag],
            },
        });
    }

    // The room pass is complete, so this snapshot is exactly "floor that belongs to a room". Corridors are
    // carved site-centre to site-centre, which means their paths run straight *through* room interiors — so
    // "corridor cell" cannot be "floor outside a room rect". It is "floor the corridor pass added that the
    // room pass had not already set", and this is the only moment that distinction is observable.
    let room_floor = walkable.clone();
    let mut corridor_of = vec![NO_CORRIDOR; width * height];

    // Corridor pass — each adjacency edge draws its own width in `[corridor_width, corridor_width_max]`
    // (uniform, from the carve RNG in adjacency order) so passages vary from tight to broad instead of
    // being identical. The drawn width is stashed per unordered edge so the necking pass below can reuse
    // the exact same value. The draw always runs (one path); a collapsed range just yields a constant.
    let (cw_min, cw_max) = (
        config.corridor_width,
        config.corridor_width_max.unwrap_or(config.corridor_width),
    );
    let mut edge_width: HashMap<(usize, usize), usize> = HashMap::new();
    for (edge_idx, &(a, b)) in layout.adjacency.iter().enumerate() {
        let w = cw_min + rng.below(cw_max - cw_min + 1);
        edge_width.insert((a.min(b), a.max(b)), w);
        carve_corridor(
            &mut walkable,
            width,
            height,
            layout.sites[a].center,
            layout.sites[b].center,
            w,
        );
        // Claim every cell this edge just opened. The `NO_CORRIDOR` guard makes the FIRST edge to open a
        // cell its owner, so an overlap at a junction resolves deterministically in adjacency order rather
        // than by whichever edge happened to be carved last. `carve_corridor` is left untouched — it stays
        // a pure walkability carve, and the identity is recovered here by diffing against `room_floor`.
        for i in 0..walkable.len() {
            if walkable[i] && !room_floor[i] && corridor_of[i] == NO_CORRIDOR {
                corridor_of[i] = edge_idx as u32;
            }
        }
    }

    // Opening pass — record each region's adjacency + the interior wall cell where its corridor actually
    // meets the room (derived from the carved L geometry, not a dominant-axis guess), and neck the doorway
    // down to a proportional band of lanes (`doorway_width`), keeping lanes `0..doorway_w` open. Iterated
    // in sort order per site so openings/adjacency match the grid's N,E,S,W scan. `derive_opening` is
    // byte-identical to the old cell formula for axis-aligned edges.
    for (si, inc) in incident.iter().enumerate() {
        let sc = layout.sites[si].center;
        let rect = regions[si].rect;
        for &(_, nb, is_first) in inc {
            let (dir, cell) = derive_opening(sc, layout.sites[nb].center, rect, is_first);
            regions[si].adjacency.push(nb as u32);
            // Same width the corridor was carved at (looked up per unordered edge). The doorway necks
            // down from this corridor's real width — not a global constant — to a PROPORTIONAL band of
            // lanes (`doorway_width`), so a broad corridor keeps a broad mouth instead of every doorway
            // pinching to one body-width (player report 2026-07-19). `doorway_ratio` is the evolvable knob.
            let cw = edge_width
                .get(&(si.min(nb), si.max(nb)))
                .copied()
                .unwrap_or(cw_min);
            let doorway_w = doorway_width(cw, config.doorway_ratio);
            regions[si].openings.push(Opening { dir, cell, width: doorway_w });
            for lane in doorway_w as i32..cw as i32 {
                let neck = match dir {
                    E => IVec2::new(cell[0] + 1, cell[1] + lane),
                    W => IVec2::new(cell[0] - 1, cell[1] + lane),
                    N => IVec2::new(cell[0] + lane, cell[1] - 1),
                    S => IVec2::new(cell[0] + lane, cell[1] + 1),
                    _ => unreachable!(),
                };
                if neck.x >= 0
                    && neck.y >= 0
                    && (neck.x as usize) < width
                    && (neck.y as usize) < height
                {
                    walkable[neck.y as usize * width + neck.x as usize] = false;
                }
            }
        }
    }

    let spawn = layout.sites[layout.spawn_site].center;
    // The necking pass above may have un-set cells that the corridor pass opened. Their `corridor_of` entry
    // survives, which is harmless: every read goes through `is_corridor`/`corridor_id`, and both gate on
    // `is_floor` first. A necked-out doorway cell is simply not floor, so it is not a corridor cell either.
    let biome_of = resolve_biomes(layout, config, &walkable, &room_of, &corridor_of, &regions)?;
    Ok((walkable, regions, spawn, corridor_of, biome_of))
}

/// Resolve the surface biome of every fine cell **per zone, not per cell** (FVS-Q-8).
///
/// The bug this replaces: [`biome_at`] was sampled at each cell independently, so a single room's floor
/// straddled the threshold and rendered as carpet in one corner and concrete in the other. (The walls
/// were never the problem — `render.rs` already keys each wall slab, corner post and lintel on the
/// *floor cell that owns it*, so a tile and its own walls always agreed. The noise was simply
/// finer-grained than a room.) Player, 2026-07-30: *"I don't like backrooms carpets and concrete walls.
/// It should be one or the other. The transition should be at a doorway."*
///
/// The rule, one per cell class, in precedence order:
/// * **Room floor** → the noise sampled once at that room's centre cell. One draw per room, so a room is
///   uniform by construction rather than probabilistically.
/// * **Corridor floor** → the biome of its lower-`RegionId` endpoint room. Every transition therefore
///   lands at a doorway, a corridor always matches a neighbour, and `biome_scale` keeps its meaning (how
///   likely two adjacent rooms differ) instead of becoming dead config.
/// * **Everything else** — rock, walls, cells the necking pass shut — → the nearest classified cell, by
///   one multi-source BFS. This is what finally makes [`Dungeon::biome`]'s "a wall belongs to the biome
///   of the cell it is attached to" true *by construction* rather than probabilistically.
///
/// Room floor deliberately outranks corridor: corridors are carved straight through room interiors, and
/// a passage crossing a hall must not restripe the hall. That precedence is free — the corridor pass only
/// claims cells the room pass had not already set.
///
/// **Draws no RNG.** [`biome_at`] is a pure hash of `(seed, cell)` and the seed is derived from the config
/// seed rather than taken from the carve stream, so moving the sample point from every cell to one cell
/// per room cannot shift a single subsequent carve draw. The layout goldens are untouched by construction.
///
/// Contrast with the landscape literature, which blends: **AutoBiomes** (Fischer, Dittmann, Weller &
/// Zachmann, *Vis. Comput.* 2020, doi 10.1007/s00371-020-01920-7) weights adjacent biomes through a
/// convolution kernel, which is right for open terrain where a transition is continuous and has no
/// architectural feature to hide the seam. An interior has one: the doorway. So the switch here is
/// deliberately **discrete**, and placed at the threshold. Room-as-unit-of-assignment is the standard
/// move in the dungeon-graph framing surveyed by Viana & Dos Santos (*J. Interact. Syst.* 2021,
/// doi 10.5753/jis.2021.999).
fn resolve_biomes(
    layout: &CoarseLayout,
    config: &DungeonConfig,
    walkable: &[bool],
    room_of: &[u32],
    corridor_of: &[u32],
    regions: &[Region],
) -> Result<Vec<Biome>, String> {
    let (width, height) = (layout.width, layout.height);
    let (seed, mix, scale) = biome_field(config);

    // One draw per room, at its centre cell — the sample point option (b) specifies.
    let room_biome: Vec<Biome> = regions
        .iter()
        .map(|r| {
            let c = r.rect.center_cell();
            biome_at(seed, IVec2::new(c[0], c[1]), mix, scale)
        })
        .collect();

    // `None` = "no zone of its own yet", resolved by the BFS below. Not a sentinel biome: a default here
    // would be a silent second path, and an unresolved cell must stay visibly unresolved.
    //
    // A room or corridor index that does not resolve is a **carver bug**, not a cell to fill in: it means
    // `room_of`/`corridor_of` disagrees with `regions`/`adjacency`. Both are in range by construction
    // (`Region.id == si`, and `corridor_of` stores an index into `layout.adjacency`), but letting a failed
    // lookup fall through to the BFS would turn that bug into a plausible-looking floor with a neighbour's
    // surface — silent, and exactly the degraded substitute the one-path rule forbids. So it errors.
    let mut out: Vec<Option<Biome>> = vec![None; width * height];
    for (i, slot) in out.iter_mut().enumerate() {
        if !walkable[i] {
            continue;
        }
        if room_of[i] != NO_ROOM {
            let r = room_of[i] as usize;
            let b = room_biome.get(r).copied().ok_or_else(|| {
                format!("dungeon: cell ({}, {}) claims room {r}, which has no region", i % width, i / width)
            })?;
            *slot = Some(b);
        } else if corridor_of[i] != NO_CORRIDOR {
            let e = corridor_of[i] as usize;
            // Option (b): a corridor takes the biome of its LOWER-`RegionId` endpoint room, so a passage
            // always matches one of the rooms it joins and the change of surface lands at a doorway.
            let &(a, b) = layout.adjacency.get(e).ok_or_else(|| {
                format!("dungeon: cell ({}, {}) claims corridor edge {e}, which is not in the adjacency graph", i % width, i / width)
            })?;
            let endpoint = a.min(b);
            let biome = room_biome.get(endpoint).copied().ok_or_else(|| {
                format!("dungeon: corridor edge {e} names endpoint room {endpoint}, which has no region")
            })?;
            *slot = Some(biome);
        }
    }

    // Multi-source BFS out from every classified cell. Determinism comes from the enumeration order, not
    // from a value sort: sources are seeded row-major and neighbours are visited in a fixed N,E,S,W order,
    // so the first writer of any cell is a function of the grid alone. Nothing here reads an ECS query.
    let mut frontier: std::collections::VecDeque<usize> = out
        .iter()
        .enumerate()
        .filter_map(|(i, b)| b.map(|_| i))
        .collect();
    if frontier.is_empty() {
        return Err("dungeon: no cell could be assigned a biome — the carve produced no floor".into());
    }
    while let Some(i) = frontier.pop_front() {
        let Some(here) = out[i] else { continue };
        let (x, y) = ((i % width) as i32, (i / width) as i32);
        for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                continue;
            }
            let n = ny as usize * width + nx as usize;
            if out[n].is_none() {
                out[n] = Some(here);
                frontier.push_back(n);
            }
        }
    }

    // The BFS reaches every cell of a connected grid from a non-empty source set, so a `None` here means
    // the grid is disconnected in a way the carve is supposed to have already rejected. Fail loud.
    out.into_iter()
        .enumerate()
        .map(|(i, b)| {
            b.ok_or_else(|| {
                format!(
                    "dungeon: cell ({}, {}) is unreachable from any floor, so it has no biome",
                    i % width,
                    i / width
                )
            })
        })
        .collect()
}

/// Doorway width (open lanes) for a corridor carved `cw` tiles wide, given the evolvable `ratio`.
/// Proportional so a broad corridor keeps a broad mouth and a tight one stays tight, clamped to
/// `1..=cw` — at least one lane is always open, and the mouth never exceeds the corridor's carved width.
/// This is the fix for every doorway being pinned to a single body-width regardless of corridor width
/// (player report 2026-07-19). `ratio` is surfaced to the QD level search as `DungeonConfig::doorway_ratio`;
/// exposing generation structure through a weight parameter is the WFC-tuning idiom (Kim, Hahn, Kim &
/// Kang, "Graph-Based Wave Function Collapse Algorithm for Procedural Content Generation in Games",
/// IEICE Trans. Inf. & Syst. 2020, doi 10.1587/transinf.2019edp7295).
pub(crate) fn doorway_width(cw: usize, ratio: f32) -> usize {
    ((cw as f32 * ratio).round() as usize).clamp(1, cw.max(1))
}

/// serde default for [`DungeonConfig::doorway_ratio`]. Ships wider than the old hard-pinned 1-lane
/// mouth: at `0.5`, a 2-wide corridor still necks to 1 but a 4–5-wide corridor opens a 2–3-lane
/// doorway, so passage width finally varies across the map instead of every mouth being one body wide.
pub(crate) fn default_doorway_ratio() -> f32 {
    0.5
}

/// Carve a `lanes`-wide corridor between two site centres. Axis-aligned edges (always, for the grid) are
/// a single straight run — lanes stack +y for a horizontal corridor, +x for a vertical one, byte-
/// identical to the pre-refactor E/S carve. Diagonal graph routes carve an L (both legs), keeping each
/// room-mouth segment axis-aligned so necking/openings still apply. Writes are bounds-checked (a no-op
/// for in-bounds grid corridors) so a wide/edge diagonal can never index out of the mask.
fn carve_corridor(
    walkable: &mut [bool],
    width: usize,
    height: usize,
    a: IVec2,
    b: IVec2,
    lanes: usize,
) {
    let carve_h = |walkable: &mut [bool], x0: i32, x1: i32, y: i32| {
        for x in x0.min(x1)..=x0.max(x1) {
            for lane in 0..lanes as i32 {
                let (px, py) = (x, y + lane);
                if px >= 0 && py >= 0 && (px as usize) < width && (py as usize) < height {
                    walkable[py as usize * width + px as usize] = true;
                }
            }
        }
    };
    let carve_v = |walkable: &mut [bool], y0: i32, y1: i32, x: i32| {
        for y in y0.min(y1)..=y0.max(y1) {
            for lane in 0..lanes as i32 {
                let (px, py) = (x + lane, y);
                if px >= 0 && py >= 0 && (px as usize) < width && (py as usize) < height {
                    walkable[py as usize * width + px as usize] = true;
                }
            }
        }
    };
    if a.y == b.y {
        carve_h(walkable, a.x, b.x, a.y);
    } else if a.x == b.x {
        carve_v(walkable, a.y, b.y, a.x);
    } else {
        // Diagonal (graph only): an L via the corner (b.x, a.y) — the horizontal leg leaves `a`'s wall,
        // the vertical leg enters `b`'s wall, both axis-aligned. The grid never reaches this branch.
        carve_h(walkable, a.x, b.x, a.y);
        carve_v(walkable, a.y, b.y, b.x);
    }
}

// ── Graph topology front-end ─────────────────────────────────────────────────────────────────────
// Poisson-disk sites → Bowyer–Watson Delaunay → degree-≤5 prune (`geom`) → `wfc::collapse_graph` decides
// which edges are corridors → keep the largest linked component → a `CoarseLayout` for `expand_to_fine`.

/// Build the Graph topology's `CoarseLayout`. Fails loud (one path) if the sites are too sparse to
/// sample or no collapse connects at least half of them (a rock-heavy roll can strand rooms).
pub(crate) fn graph_layout(
    config: &DungeonConfig,
    site_spacing: f32,
    link_weights: &[f64],
) -> Result<CoarseLayout, String> {
    let (width, height) = (
        config.coarse_w * config.block,
        config.coarse_h * config.block,
    );

    // Poisson sites, from their own RNG sub-stream (independent of the carve RNG). The rect is inset by
    // a small margin so every site sits at least `ROOM_FLOOR + 1` tiles from the level edge — that lets
    // `build_graph_layout` give each site a *symmetric*, in-bounds bounds box, which keeps `site.center`
    // interior to its room (the invariant the corridor/opening pass depends on).
    let margin = (ROOM_FLOOR + 1) as f64;
    let (inset_w, inset_h) = (
        (width as f64 - 2.0 * margin).max(1.0),
        (height as f64 - 2.0 * margin).max(1.0),
    );
    let mut site_rng = seeded(config.seed ^ 0x517E_5EED);
    let mut points = geom::poisson_disk(inset_w, inset_h, site_spacing as f64, 30, &mut site_rng);
    for p in &mut points {
        p[0] += margin;
        p[1] += margin;
    }
    let n = points.len();
    if n < 2 {
        return Err(format!(
            "graph topology: Poisson sampling produced {n} site(s) — site_spacing {site_spacing} is \
             too large for the {width}x{height} level"
        ));
    }

    // Delaunay graph, pruned to the collapse's degree cap, as port-indexed adjacency.
    let edges = geom::prune_to_max_degree(&points, &geom::delaunay_edges(&points), wfc::MAX_DEGREE);
    let neighbors = port_neighbors(n, &edges);

    // Collapse which edges are corridors, retrying with offset seeds until the largest linked component
    // covers at least half the sites — else fail loud rather than ship a mostly-isolated dungeon.
    let need = n.div_ceil(2).max(1);
    for attempt in 0..config.max_attempts.max(1) {
        let seed = (config.seed ^ 0xC011_AB5E).wrapping_add(attempt as u64);
        let Some(pattern) = wfc::collapse_graph(&neighbors, link_weights, seed) else {
            continue; // only a malformed table returns None; the prune guarantees it won't
        };
        let links = corridor_edges(&neighbors, &pattern);
        let kept = largest_graph_component(n, &links);
        if kept.len() >= need {
            return Ok(build_graph_layout(&points, &links, &kept, width, height));
        }
    }
    Err(format!(
        "graph topology: no collapse connected at least {need} of {n} sites after {} attempts; \
         raise link_weights or lower site_spacing",
        config.max_attempts.max(1)
    ))
}

/// Turn an undirected edge set into port-indexed adjacency for `wfc::collapse_graph`: each node's
/// neighbours sorted (a deterministic port order), every edge present at both endpoints with swapped
/// ports so the socket rule can pair them.
fn port_neighbors(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<(usize, usize)>> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        adj[a].push(b);
        adj[b].push(a);
    }
    for nb in &mut adj {
        // SORT-OK: seeded generator, not an ECS query.
        nb.sort_unstable();
        nb.dedup();
    }
    let mut neighbors: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for a in 0..n {
        for &b in &adj[a] {
            // `a`'s port to `b` is `b`'s slot in `adj[a]` (push order); `b`'s back-port is `a`'s slot in
            // `adj[b]`. `position` is always `Some` since `adj` is symmetric.
            if let Some(b_port) = adj[b].iter().position(|&x| x == a) {
                neighbors[a].push((b, b_port));
            }
        }
    }
    neighbors
}

/// The undirected corridor edges implied by a collapse result: `a`'s port `p → b` is a corridor iff bit
/// `p` of `pattern[a]` is set (the socket rule guarantees `b` agrees). Counted once (`a < b`).
fn corridor_edges(neighbors: &[Vec<(usize, usize)>], pattern: &[usize]) -> Vec<(usize, usize)> {
    let mut edges = Vec::new();
    for (a, ports) in neighbors.iter().enumerate() {
        for (p, &(b, _)) in ports.iter().enumerate() {
            if (pattern[a] >> p) & 1 == 1 && a < b {
                edges.push((a, b));
            }
        }
    }
    edges
}

/// The largest connected component of a site graph (given its corridor edges), as a sorted node list.
/// Isolated sites are size-1 components, so the largest linked cluster wins — the graph analogue of
/// `largest_room_component`.
fn largest_graph_component(n: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        adj[a].push(b);
        adj[b].push(a);
    }
    let mut visited = vec![false; n];
    let mut best: Vec<usize> = Vec::new();
    for start in 0..n {
        if visited[start] {
            continue;
        }
        let mut comp = Vec::new();
        let mut stack = vec![start];
        visited[start] = true;
        while let Some(u) = stack.pop() {
            comp.push(u);
            for &v in &adj[u] {
                if !visited[v] {
                    visited[v] = true;
                    stack.push(v);
                }
            }
        }
        if comp.len() > best.len() {
            best = comp;
        }
    }
    // SORT-OK: seeded generator, not an ECS query.
    best.sort_unstable();
    best
}

/// Assemble a `CoarseLayout` from the kept sites. Each site's centre rounds to the fine grid; its bounds
/// are a square, symmetric around the centre, sized to the smaller of half the nearest-neighbour
/// Chebyshev distance (so rooms provably never overlap — `h_i + h_j ≤ Cheb(i,j)`) and the distance to the
/// nearest level edge (so it stays in-bounds without breaking symmetry — the centre stays interior).
/// Adjacency is remapped to kept indices; spawn is the kept site nearest the level centre.
fn build_graph_layout(
    points: &[Point],
    links: &[(usize, usize)],
    kept: &[usize],
    width: usize,
    height: usize,
) -> CoarseLayout {
    let mut old_to_new = vec![None; points.len()];
    for (new_i, &old) in kept.iter().enumerate() {
        old_to_new[old] = Some(new_i);
    }

    let sites: Vec<Site> = kept
        .iter()
        .map(|&old| {
            let c = points[old];
            let mut min_cheb = f64::MAX;
            for &other in kept {
                if other != old {
                    let o = points[other];
                    min_cheb = min_cheb.min((o[0] - c[0]).abs().max((o[1] - c[1]).abs()));
                }
            }
            let (cx, cy) = (c[0].round() as i32, c[1].round() as i32);
            // Symmetric half-side: the smaller of half the nearest-neighbour Chebyshev distance (so no two
            // rooms overlap — `h_i + h_j ≤ Cheb(i,j)`) and the distance to the nearest level edge (so the
            // box stays symmetric around the centre AND in-bounds — keeping `site.center` interior to any
            // room centred in it, which the corridor pass relies on). A lone kept site has
            // `min_cheb == f64::MAX`; the edge terms bound it to a finite box, so there is no `i32`
            // overflow. The Poisson inset guarantees `edge ≥ ROOM_FLOOR`, so `.max(ROOM_FLOOR)` never
            // pushes the box out of bounds.
            let edge = cx.min(width as i32 - cx).min(cy).min(height as i32 - cy);
            let h = ((0.5 * min_cheb).min(edge as f64) as i32).max(ROOM_FLOOR as i32);
            Site {
                center: IVec2::new(cx, cy),
                bounds: Rect2 {
                    min: [cx - h, cy - h],
                    max: [cx + h, cy + h],
                },
            }
        })
        .collect();

    let mut adjacency: Vec<(usize, usize)> = Vec::new();
    for &(a, b) in links {
        if let (Some(na), Some(nb)) = (old_to_new[a], old_to_new[b]) {
            adjacency.push((na, nb));
        }
    }

    let center = Vec2::new(width as f32 / 2.0, height as f32 / 2.0);
    let spawn_site = (0..sites.len())
        .min_by(|&a, &b| {
            let pa = Vec2::new(sites[a].center.x as f32, sites[a].center.y as f32);
            let pb = Vec2::new(sites[b].center.x as f32, sites[b].center.y as f32);
            (pa - center)
                .length_squared()
                .total_cmp(&(pb - center).length_squared())
        })
        .unwrap_or(0); // kept is non-empty (need >= 1), so always Some

    CoarseLayout {
        width,
        height,
        sites,
        adjacency,
        spawn_site,
    }
}
