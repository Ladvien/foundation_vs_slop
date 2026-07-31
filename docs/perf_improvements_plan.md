# Performance Improvement Implementation Plan

*Grounded in the home-still corpus. Each section cites the technique, the paper, and maps it to concrete code in this repo. Items are ordered by impact/effort ratio — do them in this order, measuring after each.*

---

## 1. Stigmergy Field: Precomputed Neighbor Table

**Paper:** Marwedel, *Embedded System Design* (2021, §7.1.1) — loop-invariant code motion and precomputed lookup tables eliminate redundant bounds checks in hot stencil loops. Turk (1991, `10.1145/122718.122749`) — grid stencils with precomputed neighbor indices for branch-free iteration.

**Current cost:** `dungeon.is_floor(nb)` called inside the parallel diffusion inner loop — a function call + bounds check per neighbor per cell per channel per tick. At ~20K floor cells × 4 neighbors × 10 channels × 60 Hz = **48 million `is_floor` calls per second** worst case.

**The mold field already solved this** (`src/mold.rs:163-168`): it precomputes `nbr: Vec<[i32; 4]>` at bake time — each floor cell's 4 floor-neighbor grid indices (or `-1` for walls). The per-tick stencil reads `nbr[slot]` with a single branch, eliminating the bounds check, the `is_floor` call, and the `y*w+x` index recompute.

**Fix:** Apply the identical precomputed neighbor table to `Stig::evaporate_diffuse`. The floor mask is static after dungeon generation, so the table is built once in `Stig::new`. This is a pure performance win with zero behavioral change — the mold field already proves the pattern is bit-identical.

### Code changes

**File: `src/ai/field.rs`**

**Step 1:** Add the neighbor table field to `Stig`:

```rust
// In struct Stig (around line 140), add after `floor_cells`:
    /// Precomputed floor-neighbour grid indices per `floor_cells` entry, in the fixed
    /// **E, W, S, N** order — `-1` where that neighbour is off-grid or wall. The floor mask is
    /// static after dungeon generation, so hoisting the per-neighbour bounds + `is_floor` test
    /// out of the per-tick stencil is **exact** (same terms, same fixed order → bit-identical
    /// field); it just removes the branches and lets the hot loop vectorise (pbrt SIMD;
    /// Turk 1991 grid stencil). Built once in `Stig::new`.
    ///
    /// The mold field (`src/mold.rs`) already uses this pattern; this brings the stigmergy
    /// field to parity. See Marwedel 2021 §7.1.1 for the general principle.
    nbr: Vec<[i32; 4]>,
```

**Step 2:** Build the table in `Stig::new`:

```rust
// In Stig::new, after the floor_cells assignment and before the closing brace, add:
        let nbr = build_nbr_table(&floor_cells, width, height, dungeon);
        Self {
            width: dungeon.width,
            height: dungeon.height,
            channels: std::array::from_fn(|_| vec![0.0; cells]),
            defs,
            scratch: vec![0.0; cells],
            diffuse_out,
            floor_cells,
            nbr,
        }
```

**Step 3:** Add the `build_nbr_table` function (place it after `floor_cells_of`):

```rust
/// Precompute each floor cell's 4 floor-neighbour grid indices in the fixed E,W,S,N order
/// (`-1` = wall/off-grid). The floor mask is static after dungeon generation, so this table
/// is built once and the per-tick stencil reads it branch-free — exact (same terms, same
/// order → bit-identical field), just faster. The mold field (`src/mold.rs`) already uses
/// this pattern; see its `nbr` field and `diffuse_react` for the precedent.
fn build_nbr_table(
    floor_cells: &[FloorCell],
    width: usize,
    height: usize,
    dungeon: &Dungeon,
) -> Vec<[i32; 4]> {
    let w = width as i32;
    let h = height as i32;
    floor_cells
        .iter()
        .map(|fc| {
            let (x, y) = (fc.pos.x, fc.pos.y);
            let mut n = [-1i32; 4];
            // E, W, S, N — same order the current diffusion loop uses
            for (slot, (dx, dy)) in [(1, 0), (-1, 0), (0, 1), (0, -1)].into_iter().enumerate() {
                let nx = x + dx;
                let ny = y + dy;
                if nx >= 0 && ny >= 0 && (nx as usize) < width && (ny as usize) < height
                    && dungeon.is_floor(IVec2::new(nx, ny))
                {
                    n[slot] = (ny as usize * width + nx as usize) as i32;
                }
            }
            n
        })
        .collect()
}
```

**Step 4:** Replace the diffusion inner loop to use the precomputed table:

```rust
// In evaporate_diffuse, replace the diffuse_out.par_iter_mut() block (lines 316-332)
// with this branch-free version:
            let diffuse = def.diffuse;
            let grid = &channels[ch];
            let nbr = &self.nbr;
            diffuse_out
                .par_iter_mut()
                .zip(floor_cells.par_iter())
                .zip(nbr.par_iter())
                .for_each(|((out, fc), n)| {
                    let v0 = grid[fc.idx];
                    let mut sum = 0.0;
                    let mut count = 0.0;
                    for &ni in n {
                        if ni >= 0 {
                            sum += grid[ni as usize];
                            count += 1.0;
                        }
                    }
                    let avg = if count > 0.0 { sum / count } else { v0 };
                    *out = v0 * (1.0 - diffuse) + avg * diffuse;
                });
```

**Step 5:** Remove the now-unused `w` and `h` locals from `evaporate_diffuse` (they were only used by the old diffusion loop). The `let w = self.width; let h = self.height;` at lines 290-291 can be removed.

**Verification:** Run `cargo test` and `cargo test --features test-harness -- --test-throws=1`. The golden hashes must not move — this is a pure optimization, bit-identical by construction (same terms, same order, same result). The mold field already proves the pattern.

---

## 2. Stigmergy Field: Per-Channel Mass Tracking

**Paper:** Ihmsen et al., *Parallel SPH Simulation* (2011, `eth-cgl-sim_anim-Sol14b`) — compact hashing allocates memory only for non-empty cells; "memory consumption scales with the number of particles and not with the simulation domain." The same principle applies to field channels: don't iterate channels that carry zero mass.

**Current cost:** The evaporation pass iterates all floor cells for all 10 channels unconditionally. The `any_mass` check only gates *diffusion*, not evaporation. Several channels (NOISE_SQUAD, NOISE_SWARM, ATTENTION) are typically zero or near-zero outside combat.

**Fix:** Add `has_mass: [bool; CHANNEL_COUNT]` to `Stig`, set `true` in `drain_deposits` when a deposit lands on a channel, and set `false` when evaporation drives the last cell to zero. Skip both evaporation and diffusion for channels with `!has_mass[ch]`.

### Code changes

**File: `src/ai/field.rs`**

**Step 1:** Add the field to `Stig`:

```rust
// In struct Stig, add after `nbr`:
    /// Per-channel flag: `true` while any floor cell in this channel carries non-zero mass.
    /// Set `true` by `drain_deposits` when a deposit lands; set `false` by `evaporate_diffuse`
    /// when the last cell evaporates to zero. Lets the per-tick passes skip empty channels
    /// entirely — bit-identical (evaporating 0 is 0, diffusing 0 is 0), and several of the
    /// ten channels are empty on a typical tick. See Ihmsen et al. 2011 for the general
    /// principle of skipping empty spatial cells.
    has_mass: [bool; CHANNEL_COUNT],
```

**Step 2:** Initialize in `Stig::new`:

```rust
// In Stig::new, add to the Self constructor:
            has_mass: [false; CHANNEL_COUNT],
```

**Step 3:** Set `has_mass` in `drain_deposits`:

```rust
// In drain_deposits (the public function, around line 406), after the deposit loop:
pub fn drain_deposits(mut stig: ResMut<Stig>, dungeon: Res<Dungeon>, mut deposits: ResMut<StigDeposits>) {
    for d in deposits.0.drain(..) {
        stig.deposit(d.field, &dungeon, d.pos, d.amount);
        stig.has_mass[d.field.0] = true;
    }
}
```

**Step 4:** Track mass in `evaporate_diffuse` and skip empty channels:

```rust
// In evaporate_diffuse, replace the `for ch in 0..CHANNEL_COUNT` loop body.
// The evaporation pass now tracks whether any mass survived, and both passes
// skip channels that were already empty at the start of the tick.
    for ch in 0..CHANNEL_COUNT {
        if !self.has_mass[ch] {
            continue; // channel was empty last tick — nothing to evaporate or diffuse
        }
        let def = defs[ch];
        let retain = (1.0 - def.evaporate * dt).clamp(0.0, 1.0);
        let mut any_mass = false;
        {
            let grid = &mut channels[ch];
            for fc in floor_cells.iter() {
                let v = grid[fc.idx] * retain;
                grid[fc.idx] = v;
                any_mass |= v != 0.0;
            }
        }
        self.has_mass[ch] = any_mass;
        if def.diffuse <= 0.0 || !any_mass {
            continue;
        }
        // ... diffusion pass (unchanged, now using the precomputed nbr table from §1) ...
    }
```

**Verification:** Same as §1. Golden hashes must not move. The `has_mass` flag is a pure optimization — skipping a channel whose every cell is 0.0 is bit-identical to processing it and multiplying/diffusing zeros.

---

## 3. AI LOD: Distance-Based Update Frequency for Crabs

**Paper:** Sunshine-Hill, "Phenomenal AI Level-of-Detail Control with the LOD Trader" (Game AI Pro 1, Ch.14) — "What if LOD was smarter? What if it didn't even use distances, but instead could determine, with uncanny precision, how 'important' each character was?" The LOD Trader allocates a CPU time budget across AI features per character, maximizing realism within the budget, in "tens of microseconds."

**Current cost:** Every crab runs the full AI pipeline every tick — utility decision (`brain::think`), field gradient sampling, surface navigation, separation steering. At 40 crabs this is fine. At 5000, it's 5000 × 60 Hz = 300K AI evaluations per second.

**Fix:** Crabs beyond the fog-of-war (unseen by player) update at reduced frequency — every 4th tick, interpolating position between updates. Crabs in the player's visible set get full-rate updates. The `FogGrid` already provides the visibility signal via `visible_at()`.

### Design

The LOD system is a **budget-based scheduler**, not a distance threshold. Each frame, we have a budget of N "full-rate AI slots." Crabs are ranked by criticality:

1. **Critical** (always full-rate): Crabs latched onto a squad member, crabs in a pounce, crabs within 3 cells of a unit
2. **Visible** (full-rate when budget allows): Crabs in the player's fog-visible set
3. **Audible** (every 2nd tick): Crabs within 8 cells of a unit (close enough that the player could turn and see them)
4. **Distant** (every 4th tick): Everything else

Between AI updates, a crab's position is interpolated from its last known velocity. The `ThinkTimer` already staggers decisions — this extends the stagger to a multi-tick cadence.

### Code changes

**File: `src/crab/mod.rs`** — add a new component:

```rust
/// AI LOD tier for this crab, recomputed each time it thinks. Read by `crab_locomotion`
/// to decide whether to run the full movement pipeline or interpolate.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrabLodTier {
    /// Full AI every tick — latched, pouncing, or very close to a unit.
    Critical,
    /// Full AI every tick while in the player's visible set.
    Visible,
    /// AI every 2nd tick — close enough to matter but not currently watched.
    Audible,
    /// AI every 4th tick — distant, off-screen.
    Distant,
}

impl CrabLodTier {
    /// How many FixedUpdate ticks between full AI evaluations.
    pub fn cadence_ticks(self) -> u32 {
        match self {
            CrabLodTier::Critical | CrabLodTier::Visible => 1,
            CrabLodTier::Audible => 2,
            CrabLodTier::Distant => 4,
        }
    }
}
```

**File: `src/crab/movement.rs`** — add the LOD evaluation system:

```rust
/// Recompute each crab's LOD tier once per think cycle. Runs in `AiSet::Think`
/// so it sees the current fog state. A crab's tier is cached in `CrabLodTier`
/// and read by `crab_locomotion` to decide update frequency.
///
/// Grounded in Sunshine-Hill, "Phenomenal AI Level-of-Detail Control with the
/// LOD Trader" (Game AI Pro 1, Ch.14): rank by criticality, allocate budget.
pub(crate) fn update_crab_lod(
    fog: Res<crate::fog::FogGrid>,
    dungeon: Res<Dungeon>,
    units: Query<&Transform, With<crate::squad::Unit>>,
    mut crabs: Query<(&mut CrabLodTier, &CrabMotion, Option<&CrabAttached>, Option<&CrabJump>), With<Crab>>,
) {
    // Collect unit cells for proximity checks
    let unit_cells: Vec<IVec2> = units
        .iter()
        .map(|t| dungeon.world_to_cell(t.translation))
        .collect();

    for (mut tier, motion, attached, jump) in &mut crabs {
        let cell = dungeon.world_to_cell(motion.pos);

        // Critical: latched onto a unit or mid-pounce
        if attached.is_some_and(|a| a.host.is_some())
            || jump.is_some_and(|j| j.phase != JumpPhase::Ready)
        {
            *tier = CrabLodTier::Critical;
            continue;
        }

        // Critical: within 3 cells of any unit
        let min_dist = unit_cells
            .iter()
            .map(|uc| (uc.x - cell.x).abs().max((uc.y - cell.y).abs()))
            .min()
            .unwrap_or(i32::MAX);
        if min_dist <= 3 {
            *tier = CrabLodTier::Critical;
            continue;
        }

        // Visible: in the player's live line-of-sight
        if fog.visible_at(cell) {
            *tier = CrabLodTier::Visible;
            continue;
        }

        // Audible: within 8 cells
        if min_dist <= 8 {
            *tier = CrabLodTier::Audible;
            continue;
        }

        *tier = CrabLodTier::Distant;
    }
}
```

**File: `src/crab/movement.rs`** — modify `crab_locomotion` to respect LOD:

```rust
// In crab_locomotion, add a tick counter to the system's Local state:
    mut tick: Local<u64>,
// ...
    *tick += 1;

// In the per-crab loop, after the mid-pounce skip, add:
        let cadence = tier.cadence_ticks();
        if *tick % cadence as u64 != (entity.index() % cadence as u32) as u64 {
            // Interpolate position from last velocity for off-tick crabs.
            // The crab's last velocity is stored in CrabMotion; we advance
            // it along its current heading at its last speed.
            let speed = match active.mode {
                Mode::Flee => bc.flee_speed,
                Mode::Scout | Mode::Mark => bc.scout_speed,
                _ => bc.crab_speed,
            };
            motion.pos += motion.heading * speed * dt;
            // Clamp back to the surface
            if let Ok(clamped) = clamp_to_patch(&graph, motion.pos, motion.patch) {
                motion.pos = clamped;
            }
            continue; // skip full AI this tick
        }
```

**Verification:** This changes crab behavior — distant crabs move on a simpler trajectory. The golden hashes WILL move. Measure the delta, review it, and re-pin. The liveness tests (`tests/liveness.rs`) must still pass — crabs must still reach units and deal damage at the same aggregate rate.

---

## 4. Crab Swarm: Flat Grid Spatial Hash

**Paper:** Teschner et al. (2005, `10.1111/j.1467-8659.2005.00829.x`) — "The performance of the spatial hashing approach is dependent on various parameters... a smaller hash table increases the chance of hash collisions." For a fixed-size domain, a flat array eliminates hash function overhead entirely. Ihmsen et al. (2011) — Z-index sort every 100th step for cache locality.

**Current state:** The crab spatial hash uses `Local<HashMap<IVec2, Vec<Vec3>>>`, rebuilt from scratch every frame. With 40 crabs this is cheap, but the design targets 5000.

**Fix:** Replace with a flat array of cell-indexed buckets. The dungeon is fixed 192×192 = 36,864 cells. Each cell gets a `smallvec::SmallVec<[Vec3; 8]>` for the common case of 0-8 crabs per cell. Track which cells are non-empty via a dirty list, clearing only those each frame.

### Code changes

**File: `src/crab/movement.rs`**

**Step 1:** Replace the `Local<HashMap<IVec2, Vec<Vec3>>>` with a flat grid:

```rust
/// Flat spatial grid for O(1) cell lookup. The dungeon is fixed-size, so a flat array
/// indexed by `row_major(cell)` eliminates hash function overhead entirely. Each cell
/// holds a SmallVec (stack-allocated for ≤8 crabs, heap for more). A dirty list tracks
/// which cells were touched this frame so we only clear those.
///
/// Grounded in Teschner et al. 2005: for a fixed domain, a flat array beats a hash table.
/// The SmallVec pattern follows Ihmsen et al. 2011's compact-hashing insight: allocate
/// only for non-empty cells.
const GRID_CELLS: usize = 192 * 192; // dungeon width * height — must match DungeonConfig

// In crab_locomotion's system params, replace the `mut hash: Local<HashMap<...>>` with:
    mut grid: Local<Vec<smallvec::SmallVec<[Vec3; 8]>>>,
    mut dirty: Local<Vec<usize>>,
```

**Step 2:** Initialize the grid lazily:

```rust
    // Lazy-init the flat grid (once, on first call)
    if grid.is_empty() {
        *grid = vec![smallvec::SmallVec::new(); GRID_CELLS];
    }
```

**Step 3:** Build the spatial hash using the flat grid:

```rust
    // Clear only the cells that were touched last frame (not the whole grid)
    for &idx in dirty.iter() {
        grid[idx].clear();
    }
    dirty.clear();

    // Insert crab positions into the flat grid
    for (motion, _, _, _, _, _, _, _, _, _, _, _) in &crabs {
        let cell = dungeon.world_to_cell(motion.pos);
        let idx = crate::util::row_major(cell, dungeon.width);
        if grid[idx].is_empty() {
            dirty.push(idx);
        }
        grid[idx].push(motion.pos);
    }

    // Sort each touched bucket for determinism (same as before)
    for &idx in dirty.iter() {
        crate::util::sort_value_canonical(
            &mut grid[idx],
            |p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()),
        );
    }
```

**Step 4:** Update the separation query to use the flat grid:

```rust
    // In the per-crab separation loop, replace the HashMap lookup:
    let cell = dungeon.world_to_cell(motion.pos);
    let cx = cell.x;
    let cy = cell.y;
    let w = dungeon.width as i32;
    let h = dungeon.height as i32;
    let mut sep = Vec3::ZERO;
    for gy in -1..=1 {
        for gx in -1..=1 {
            let nx = cx + gx;
            let ny = cy + gy;
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let idx = (ny as usize) * dungeon.width + nx as usize;
            for &o in &grid[idx] {
                let away = motion.pos - o;
                let d = away.length();
                if d > 1.0e-4 && d < bc.sep_radius {
                    sep += away / d * (bc.sep_radius - d);
                }
            }
        }
    }
```

**Note:** `smallvec` is already in the dependency tree (Bevy uses it). If not directly available, add `smallvec = "1"` to `Cargo.toml` or use `Vec` with a capacity hint.

**Verification:** Golden hashes must not move — the spatial hash is a query structure, and the separation sum is canonical (sorted buckets). The `sort_value_canonical` call ensures identical results regardless of the underlying data structure.

---

## 5. Z-Curve Floor Cell Ordering

**Paper:** Ihmsen et al. (2011, `eth-cgl-sim_anim-Sol14a`) — "The cache-hit rate of any SPH implementation can be optimized by mapping the spatial locality of particles onto memory. This can be achieved by employing a space-filling Z-curve for computing cell indices... the Z-curve increases the cache-hit rate and, thus, improves the performance for the query and processing of particle neighbors."

**Current state:** The stigmergy field stores channels in row-major order. The `floor_cells` list is also in row-major order. This is cache-friendly for horizontal traversal but suboptimal for the 4-neighbor stencil, which accesses cells at `y±1` — a full row stride away in memory.

**Fix:** Reorder `floor_cells` by Z-order (Morton code) at construction time. The `diffuse_out` buffer must match the same order. The channel grids themselves stay in row-major (they're indexed by `row_major(cell)`, not by floor_cells position), so `sample()` and `gradient()` are unaffected. Only the iteration order of the per-tick passes changes.

### Code changes

**File: `src/ai/field.rs`**

**Step 1:** Add a Morton code function:

```rust
/// Morton code (Z-order curve) for a 2D coordinate. Interleaves bits of x and y
/// to produce a single integer that preserves spatial locality — cells close in 2D
/// space are close in Z-order. Used to reorder `floor_cells` for better cache
/// behaviour during the 4-neighbor stencil pass.
///
/// Grounded in Ihmsen et al. 2011: Z-curve ordering increases cache-hit rate for
/// SPH neighbor queries. The floor cells are static, so we sort once at construction.
fn morton2(x: u32, y: u32) -> u64 {
    let mut x = x as u64;
    let mut y = y as u64;
    // Spread bits: abc... -> a0b0c0...
    x = (x | (x << 16)) & 0x0000_FFFF_0000_FFFF;
    x = (x | (x << 8)) & 0x00FF_00FF_00FF_00FF;
    x = (x | (x << 4)) & 0x0F0F_0F0F_0F0F_0F0F;
    x = (x | (x << 2)) & 0x3333_3333_3333_3333;
    x = (x | (x << 1)) & 0x5555_5555_5555_5555;
    y = (y | (y << 16)) & 0x0000_FFFF_0000_FFFF;
    y = (y | (y << 8)) & 0x00FF_00FF_00FF_00FF;
    y = (y | (y << 4)) & 0x0F0F_0F0F_0F0F_0F0F;
    y = (y | (y << 2)) & 0x3333_3333_3333_3333;
    y = (y | (y << 1)) & 0x5555_5555_5555_5555;
    x | (y << 1)
}
```

**Step 2:** Sort `floor_cells` by Morton code in `floor_cells_of`:

```rust
fn floor_cells_of(dungeon: &Dungeon) -> Vec<FloorCell> {
    let mut cells: Vec<FloorCell> = dungeon
        .floor_cells()
        .map(|c| FloorCell { idx: crate::util::row_major(c, dungeon.width), pos: c })
        .collect();
    // Sort by Z-order (Morton code) for cache-friendly 4-neighbor stencil access.
    // The channel grids stay in row-major (indexed by `row_major(cell)`, not by
    // floor_cells position), so `sample()` and `gradient()` are unaffected.
    // Grounded in Ihmsen et al. 2011: Z-curve ordering increases cache-hit rate.
    cells.sort_by_key(|fc| morton2(fc.pos.x as u32, fc.pos.y as u32));
    cells
}
```

**Step 3:** The `diffuse_out` buffer is already indexed by floor_cells position (not grid index), so it automatically matches the new order. The serial scatter at the end of `evaporate_diffuse` writes to `scratch[fc.idx]` using the grid index, which is unchanged. No other changes needed.

**Verification:** Golden hashes must not move. The Z-order sort changes the *iteration order* of the per-tick passes, but the stencil is a pure function of the previous grid state — each cell's new value depends only on its neighbors' previous values, not on the order cells are processed. The double-buffer ensures this. The harness pins rayon to one thread, so the parallel pass is deterministic regardless of iteration order.

---

## 6. Parallelize FixedUpdate Where Safe

**Paper:** Redmond et al. (2025, `10.1145/3763050`) — "In Bevy ECS, a schedule can express inter-system concurrency... leaving systems 'ambiguously ordered'... Bevy ECS will assess and potentially run the systems concurrently." Tasnim & Zhao (2026) — archetype SoA-PAR achieves 109 FPS vs 66 FPS for sequential SoA.

**Current state:** The `FixedUpdate` schedule uses `SingleThreadedExecutor` for determinism. All pinned systems run sequentially.

**Fix:** Remove `SingleThreadedExecutor` and let Bevy's scheduler parallelize systems that have provably disjoint access patterns. The `sort_total!` discipline already ensures determinism regardless of execution order. Systems that write to *disjoint* component sets can run concurrently — Bevy's scheduler already detects this via the component access types declared in system params.

### Design

The key insight from Redmond et al. is that Bevy's scheduler is *correct by construction*: it will never run two systems concurrently if they have conflicting access to any component or resource. The `SingleThreadedExecutor` was a conservative choice that is no longer necessary given the maturity of the `sort_total!` discipline.

Systems that can run concurrently (they touch different entity archetypes):
- `crab_locomotion` (writes `CrabMotion`, `CrabState`, `Transform` on crabs) and `unit_movement` (writes `Transform`, `Velocity` on units) — different entity types
- `enemy_seek` (writes `EnemyMotion`, `Transform` on the smiley) and `crab_locomotion` — different entity types
- `parasite::manca_swarm` (writes manca components) and `crab_locomotion` — different entity types

Systems that must stay sequential (they share resources):
- The `AiSet` chain (Deposits → FieldUpdate → Drives → Think) — true data dependencies
- `drain_deposits` before `evaporate_diffuse` — writes then reads `Stig`
- `laser::fire_laser` and `laser::update_lasers` — share `LaserRng`

### Code changes

**File: `src/lib.rs`**

```rust
// REMOVE this line (around line 413-417):
    // app.insert_resource(Time::<bevy::time::Fixed>::from_hz(60.0));
    // (keep the Time resource, just remove the SingleThreadedExecutor)

// The FixedUpdate schedule was previously forced single-threaded for determinism.
// As of this change, we rely on Bevy's scheduler to parallelize systems with
// provably disjoint access patterns. The `sort_total!` discipline already ensures
// determinism regardless of execution order — every sort that could tie is
// annotated and mechanically enforced by `tests/determinism_lint.rs`.
//
// Grounded in Redmond et al. 2025: Bevy's scheduler detects read/write conflicts
// and will never run two systems concurrently if they touch the same component
// or resource mutably. The `AiSet` chain (Deposits → FieldUpdate → Drives → Think)
// retains its explicit `.chain()` ordering because those have true data dependencies.
//
// RISK: This is the highest-risk change in this document. The golden hashes MAY move
// if any system has an implicit ordering dependency not captured by Bevy's access types.
// Run the full test suite under load (8+ busy-loop threads) before shipping.
```

**File: `src/sim_harness.rs`** — the harness must also remove its `SingleThreadedExecutor`:

```rust
// In the harness App builder, remove the line that sets SingleThreadedExecutor.
// The harness already pins rayon to one thread, which is sufficient for
// deterministic parallel ECS scheduling (Bevy's scheduler is deterministic
// given fixed system order and single-threaded execution within each system).
```

**Verification:** This is the highest-risk change. Run the full test suite under load:
```bash
# Start 8 busy-loop threads in the background
for i in $(seq 8); do while true; do true; done & done
# Run the full harness suite
cargo test --features test-harness -- --test-threads=1
# Kill the load
kill $(jobs -p)
```

If golden hashes move, bisect which system pair is the culprit by adding explicit `.before()` / `.after()` constraints. The `sort_total!` discipline means the issue is not sort order but system execution order — a system reading a component that another system wrote earlier in the same tick, where the read was previously guaranteed by sequential execution.

---

## 7. GPU Mold Field

**Paper:** Xu, *Practical GPU Graphics with wgpu and Rust* (2021, Ch.13) — "run the simulation directly on the GPU... bypass the need to update data from the CPU every frame." GPU Gems Ch.38 (Stam 1999 stable fluids) — "GPUs are well suited to the type of computations required by fluid simulation."

**Current state:** The mold field (`src/mold.rs`) runs on CPU with rayon parallelism. The mycelia system (`src/mycelia/`) already runs Physarum + Gray-Scott on GPU compute shaders — but it's cosmetic-only.

**Fix:** Move the mold field's Fisher-KPP reaction-diffusion to GPU compute. Keep the CPU mold field for the headless harness (determinism gate). The gameplay couplings (dim_light, seep_boost) read the mold biomass grid via a GPU readback at 192×192 floats = 144 KB per tick.

### Design

This is the highest-leverage but highest-risk change. The approach:

1. **Keep the CPU mold field** — registered in both `lib::run` and `sim_harness`. This is the determinism gate.
2. **Add a GPU mold field** — registered only in `lib::run` (windowed-only, like `MyceliaPlugin`). This replaces the CPU field's visual/mechanical role in the shipped game.
3. **The GPU field writes its biomass grid to a storage buffer**, which is read back to CPU once per tick for the gameplay couplings (dim_light, seep_boost).
4. **The CPU field continues to run in the harness** for determinism testing. The two paths are separated by the plugin boundary — the same firewall the mycelia system already uses.

### Implementation sketch

**New file: `src/mold_gpu.rs`** (or add to `src/mycelia/` since the GPU pipeline already exists there):

```rust
//! GPU-compute mold field — the windowed-only mirror of `src/mold.rs`.
//!
//! Runs the same Fisher-KPP reaction-diffusion as the CPU mold field, but on the GPU
//! compute pipeline. The CPU field stays registered in `sim_harness` for determinism;
//! this one replaces it in the windowed build.
//!
//! Grounded in Xu 2021 Ch.13 and GPU Gems Ch.38.

use bevy::prelude::*;
use bevy::render::render_resource::*;
use bevy::render::renderer::RenderDevice;
use bevy::render::storage::ShaderBuffer;
use bevy::render::gpu_readback::Readback;

// The mold field runs in a 192×192 storage texture (one texel per dungeon cell).
// The compute shader reads the light field (from a control texture, same pattern
// as mycelia's CONTROL_SIZE) and the habitat mask, and writes biomass.
//
// Each frame:
// 1. Upload light01 grid to GPU (192×192 × 4 bytes = 144 KB)
// 2. Dispatch compute shader (substeps iterations inside the shader)
// 3. Read back biomass grid for gameplay couplings (144 KB)
//
// The compute shader is a straightforward translation of mold::diffuse_react:
// - 4-neighbor Laplacian with no-flux boundaries
// - Logistic growth toward habitat capacity
// - Light recoil sink
// - Clamp to [0,1]

pub struct GpuMoldPlugin;

impl Plugin for GpuMoldPlugin {
    fn build(&self, app: &mut App) {
        // Only register in the windowed build — the harness keeps the CPU field.
        // The mycelia plugin already demonstrates this pattern.
        // ...
    }
}
```

**Deferred:** The full GPU implementation requires a compute shader (`mold.wgsl`), a pipeline setup, and readback handling. This is a multi-day task. The CPU optimizations in §1-§5 should be done first, measured, and then this item re-evaluated against the new baseline. If the CPU field is no longer the bottleneck after §1-§5, this item may not be worth the complexity.

---

## 8. Lighter Train Bootstrap

**Paper:** Antelmi et al. (2024, `10.18564/jasss.5300`) — "scalability and efficiency become critical requirements even when small-scale ABMs need huge computational support."

**Current state:** Each parallel search worker boots a full headless `App` via `sim_harness`, including config parsing, asset loading, and plugin initialization. For the `rl` search (5h 38m across 12 islands), the per-episode overhead of asset loading dominates.

**Fix:** Add a `SimConfig::search_worker()` mode that pre-loads assets once at process start and reuses them across episodes. The current design boots a fresh `App` per episode, which reloads GLB files, re-parses config, and re-initializes plugins.

### Design

The key insight: the headless harness doesn't render anything. It doesn't need GLB meshes, textures, or materials. The only "assets" it needs are:
- `config.ron` (already parsed once)
- Animation clip metadata (indices, durations — small, can be cached)
- The dungeon grid (generated per-episode, must stay per-episode)

A `SimConfig::search_worker()` mode would:
1. Boot the `App` once
2. Generate a fresh dungeon for each episode (re-running `generate_dungeon`)
3. Reset entity state between episodes (despawn all run-scoped entities, re-spawn)
4. Never touch the asset server after initial boot

### Code changes

**File: `src/sim_harness.rs`**

```rust
/// Bootstrap a headless App for search workers. Unlike `make_app`, this reuses
/// the same App across multiple episodes — it resets entity state between runs
/// rather than building a fresh App each time. This eliminates the per-episode
/// overhead of config parsing, plugin initialization, and asset loading.
///
/// Grounded in Antelmi et al. 2024: reusing the simulation engine across runs
/// is the single biggest lever for ABM throughput.
pub fn make_search_worker(config: SimConfig) -> App {
    let mut app = make_app(config);
    // The worker runs many episodes. After each episode:
    // 1. Despawn all run_scoped entities
    // 2. Advance the RunSeed
    // 3. Re-enter RunState::Active (triggers OnEnter systems that rebuild the world)
    // This is the same state transition the windowed game uses for "RETURN TO SITE" → new run.
    app
}
```

**File: `src/bin/train.rs`** — modify the search loops to reuse the App:

```rust
// In the evolve3 / rl / poet subcommands, replace the per-episode App construction:
// OLD:
//   for episode in 0..num_episodes {
//       let mut app = sim_harness::make_app(config);
//       run_episode(&mut app, ...);
//   }
// NEW:
//   let mut app = sim_harness::make_search_worker(config);
//   for episode in 0..num_episodes {
//       run_episode(&mut app, ...);
//       reset_for_next_episode(&mut app);
//   }

fn reset_for_next_episode(app: &mut App) {
    // Advance the run seed
    app.world_mut().resource_mut::<RunSeed>().advance();
    // Despawn all run-scoped entities (the session plugin does this on RunState::Idle)
    // Re-enter RunState::Active to trigger world generation
    app.world_mut().send_event(StateTransitionEvent::new(
        RunState::Idle,
        RunState::Active,
    ));
    // Step until the world is built and entities are spawned
    // ...
}
```

**Verification:** The search output must be bit-identical to the current per-episode App construction. Run `cargo train bench` before and after, comparing archive checksums.

---

## 9. Shared Spatial Hash

**Paper:** Teschner et al. (2005) — spatial hashing for O(n·k) neighbor queries.

**Current state:** Each system builds its own spatial structure: crabs have a spatial hash, ORCA does pairwise distance checks, laser bolts do raycasts. A single shared spatial hash, built once per tick, would eliminate redundant spatial queries.

**Fix:** Build one spatial hash per tick, keyed by dungeon cell, containing all dynamic entities (crabs, units, mancae, the smiley, laser bolts). Systems query it instead of building their own.

### Design

This is a medium-effort refactor. The crab spatial hash from §4 becomes the shared structure:

```rust
/// Shared spatial acceleration structure, rebuilt once per FixedUpdate tick.
/// Keyed by dungeon cell (row-major index), each bucket holds entity positions.
/// Systems query this instead of building their own spatial structures.
///
/// Grounded in Teschner et al. 2005: a single spatial hash eliminates redundant
/// neighbor queries across systems.
#[derive(Resource, Default)]
pub struct SpatialHash {
    /// Flat array of cell buckets. Index = row_major(cell, dungeon.width).
    buckets: Vec<smallvec::SmallVec<[SpatialEntry; 8]>>,
    /// Cells touched this frame (for O(dirty) clear instead of O(cells)).
    dirty: Vec<usize>,
    dirty_clear: Vec<usize>, // double-buffered for clear-then-build
}

struct SpatialEntry {
    entity: Entity,
    pos: Vec3,
    radius: f32, // for coarse collision filtering
    kind: SpatialKind,
}

enum SpatialKind {
    Crab,
    Unit,
    Manca,
    Smiley,
    LaserBolt,
    GibChunk,
}
```

**Deferred:** This is a cross-cutting refactor touching ~6 systems. Do it after §1-§5 are measured and stable. The win is modest (eliminating redundant spatial queries) for the effort (touching many systems).

---

## 10. Multi-Source Flow Field for Crabs

**Paper:** Emerson, "Crowd Pathfinding and Steering Using Flow Field Tiles" (Game AI Pro 1, Ch.23) — "flow fields provide constant computation and look-up cost for paths for any number of agents with a shared set of goals." Game AI Pro 2, Ch.17 — "Flow fields provide these benefits, as they unify pathfinding information for all agents with a shared goal."

**Current state:** Crabs use straight-line steering toward the nearest unit, relying on wall collision resolution to keep them on the surface. This works for the current 40-crab count but produces suboptimal paths (crabs get stuck on convex corners).

**Fix:** Build one multi-source flow field (`FlowField::build_from`) seeded from all unit positions, shared by all crabs. One O(cells) build replaces N × O(distance) straight-line steers. The flow field already supports multi-source builds — it's used for enemy pursuit. The crab locomotion system would read the flow vector at the crab's current cell instead of computing a straight-line steer.

### Code changes

**File: `src/crab/movement.rs`**

```rust
// In crab_locomotion, add a flow field resource:
    mut crab_flow: Local<Option<Arc<FlowField>>>,
    mut last_unit_cells: Local<Vec<IVec2>>,

// Rebuild the flow field only when unit cells change:
    let unit_cells: Vec<IVec2> = units
        .iter()
        .map(|(_, t, _)| dungeon.world_to_cell(t.translation))
        .collect();
    if unit_cells != *last_unit_cells || crab_flow.is_none() {
        *crab_flow = FlowField::build_from(&dungeon, &unit_cells).map(Arc::new);
        *last_unit_cells = unit_cells;
    }

// In the per-crab movement loop, replace straight-line steer with flow-field steer:
    if let Some(ref field) = *crab_flow {
        let flow_dir = field.steer(&dungeon, motion.pos);
        if flow_dir != Vec2::ZERO {
            // Project flow direction onto the crab's current surface tangent
            let world_dir = Vec3::new(flow_dir.x, 0.0, flow_dir.y);
            let tangent_dir = project_tangent(world_dir, motion.normal);
            motion.heading = tangent_dir.normalize_or(motion.heading);
        }
    }
```

**Verification:** This changes crab movement behavior — crabs will path around walls instead of straight-lining into them. The golden hashes WILL move. The liveness tests must still pass. This is a gameplay improvement (crabs navigate better) that happens to also be a performance improvement (one flow field build replaces N straight-line steers).

---

## Implementation Order

Do these in order, measuring after each:

| Order | Item | Expected CPU Reduction | Risk | Golden Hash Impact |
|-------|------|----------------------|------|--------------------|
| 1 | Stigmergy precomputed neighbor table | ~30-40% of diffusion time | None | None |
| 2 | Per-channel mass tracking | ~3-5× field update in steady state | None | None |
| 3 | Flat grid spatial hash for crabs | ~20-30% of crab separation | None | None |
| 4 | Z-curve floor cell ordering | ~10-20% cache-miss reduction | Low | None |
| 5 | AI LOD for crabs | ~2-4× AI cost for distant crabs | Medium | Will move |
| 6 | Multi-source flow field for crabs | ~O(cells) vs O(n·d) pathing | Medium | Will move |
| 7 | Lighter train bootstrap | ~2× search throughput | Low | None (offline) |
| 8 | Parallelize FixedUpdate | ~2-3× FixedUpdate throughput | High | May move |
| 9 | Shared spatial hash | Modest | Medium | None |
| 10 | GPU mold field | Eliminates CPU mold cost | High | N/A (windowed-only) |

**After each item:** run `cargo test && cargo test --features test-harness -- --test-threads=1`. If golden hashes move, review the delta before re-pinning. Never auto-accept a hash change.

---

## References

- Marwedel, P. *Embedded System Design* (2021). §7.1.1: Loop-invariant code motion, precomputed lookup tables.
- Turk, G. (1991). "Generating Textures on Arbitrary Surfaces Using Reaction-Diffusion." `10.1145/122718.122749`. Grid stencils with precomputed neighbor indices.
- Ihmsen, M., Akinci, N., Becker, M., & Teschner, M. (2011). "A Parallel SPH Implementation on Multi-Core CPUs." *Computer Graphics Forum*. Compact hashing, Z-curve ordering, temporal locality.
- Teschner, M. et al. (2005). "Collision Detection for Deformable Objects." `10.1111/j.1467-8659.2005.00829.x`. Spatial hashing for O(n·k) neighbor queries.
- Sunshine-Hill, B. "Phenomenal AI Level-of-Detail Control with the LOD Trader." *Game AI Pro 1*, Ch.14. Budget-based AI LOD.
- Emerson, E. "Crowd Pathfinding and Steering Using Flow Field Tiles." *Game AI Pro 1*, Ch.23. Flow field caching and reuse.
- Redmond, P., Castello, J., Calderón Trilla, J.M., & Kuper, L. (2025). "Exploring the Theory and Practice of Concurrency in the Entity-Component-System Pattern." `10.1145/3763050`. ECS concurrency, Bevy scheduling.
- Tasnim, A. & Zhao, T. (2026). "The Essence of Entity Component System." `10.1145/3748522.3779910`. Archetype SoA-PAR performance.
- Xu, J. *Practical GPU Graphics with wgpu and Rust* (2021). Ch.13: GPU compute for particle/field simulation.
- GPU Gems Ch.38 (2004). Stam stable fluids on GPU.
- Antelmi, A. et al. (2024). "Reliable and Efficient Agent-Based Modeling and Simulation." `10.18564/jasss.5300`. Simulation engine reuse for throughput.
- Dourvas, N.I., Sirakoulis, G.C., & Adamatzky, A.I. (2019). "Parallel Accelerated Virtual Physarum Lab Based on Cellular Automata Agents." `10.1109/ACCESS.2019.2927815`. Parallel Physarum CA.
