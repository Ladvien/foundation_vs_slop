# Dev Journal: 2026-07-06 — RON-configurable, realistic dungeon rooms (Phase 1)

**Purpose:** Phase 1 of the WFC room/hallway improvement plan — make generation RON-adjustable,
rooms realistically sized/typed, couple furniture to room type, and fix two correctness issues. Phase 2
(the liminality dial / organic layout) and Phase 3 (graph-based WFC) build on this.

## What changed

### Generation is now data-driven — `assets/dungeon.ron` + `DungeonConfig`
- New `DungeonConfig` / `WfcWeights` / `RoomType` serde structs in `src/dungeon.rs`, loaded
  **synchronously and required** in `DungeonPlugin::build` (`std::fs::read_to_string` + `ron::from_str`,
  panic-loud on missing/malformed — mirrors `PlacementPlugin` / `metropolis.ron`). `parse_config`
  validates every invariant at the door (`block >= 4`, `corridor_width in 1..=block`,
  `liminality in [0,1]`, non-empty `room_types` with positive total weight, `aspect >= 1`).
- `Dungeon::generate(seed)` → `generate(&DungeonConfig) -> Result<Self, String>`.
  `wfc::generate` / `build_prototypes` now take the 6 prototype weights.

### The const → config split (the key decision)
- **Moved to RON:** `coarse_w/h`, `block`, `corridor_width`, `seed`, `max_attempts`, the 6 WFC
  prototype weights, the room-type table, `liminality`. (Verified: zero consumers outside
  `dungeon.rs`/`wfc.rs`.)
- **Stayed compile-time `const`:** `TILE_SIZE`, `WALL_THICKNESS`, `WALL_HEIGHT`, `DOORWAY_HEIGHT`,
  `MAX_STEP`, `CAMERA_WALL_FRACTION`, … — these are a *world-physics contract* consumed by `const`
  initializers in **other** modules (`squad.rs:417 const WALK_HALF`, `metropolis.rs:29 const WALL_INSET`,
  `nest.rs:36 const NEST_WALL_HEIGHT`) plus collision/nav math across 8+ files. A `const` cannot be
  initialized from a runtime value, so runtime-izing them would fan a resource-thread across the whole
  codebase and create the exact dual-path the one-path rule forbids. No value lives in both a const and
  the RON.

### Realistic, type-driven room sizing (Merrell 2011)
- Rooms are no longer a uniform `6..14 m` square draw. `pick_room` draws a **weighted room type**, then
  a **per-type area (m²) and aspect ratio**, orients the long axis, and clamps to the block with a rock
  margin. The shipped `room_types` are real residential metric ranges (bathroom 3–6 m², bedroom 9–20,
  office 9–16, kitchen 9–18, living 16–40, hall 60–250). Determinism preserved (one seeded ChaCha8
  stream, fixed draw order).
- Each room stores its type on `Region.props.tags` (`["room", "<type>"]`) — the channel the furniture
  pass reads.

### Furniture couples to the generated room type
- `furnish::room_profile` now reads `region.props.tags` (the real, generation-time type) instead of the
  `region_id % 4` placeholder. `region_id` is retained only for the variety scan-rotation. The universal
  top-up scan stays the single furnishing path, so a room whose type has no kit match is never empty
  (no branch). Added `office` (Desk) and `bathroom` (Toilet/Sink/Bath) freestanding entries to
  `furniture.ron`. **Verified visually:** an office-typed room furnishes with a desk + monitor.

### Correctness fixes
- **Boundary invariant (`wfc.rs`):** `collapse_grid` gained a required `initial: &[u32]` per-cell domain
  (a CSP unary constraint; Karth & Smith 2017). `wfc::generate` builds it via `boundary_initial`, which
  forbids any prototype whose *off-grid* edge is a Link — so a corridor can never dead-end into the void
  at the map edge (the module's stated invariant, previously violated because propagation skips off-grid
  neighbours). Propagation was extracted into one shared `propagate` helper. Tested:
  `boundary_links_never_point_off_grid`, `initial_domains_restrict_output`.
- **No panics:** `spawn_slot`'s `partial_cmp().unwrap()` → `f32::total_cmp`; `.expect("…at least one
  room")` → `Result` + `ok_or_else`, surfaced loud in `build()`. Additionally, `Dungeon::generate` now
  **re-rolls the coarse collapse with offset seeds** until it yields ≥1 room (an all-Solid collapse is a
  valid non-contradiction WFC result that `wfc::generate` won't retry), then fails loud — so
  RON-adjustable small grids don't crash. Tested: `zero_room_config_returns_err_not_panic`.

## The liminality dial (Phase 2 — implemented)
`config.liminality ∈ [0,1]` ships at `1.0` (sparse Backrooms). Every organic perturbation is
`base + delta·(1 - liminality)` and draws no RNG at `t=0`, so `1.0` is byte-identical to Phase 1.
- **Jitter** (`jitter_origin`): slides each room off its block centre, bounded so the block centre stays
  interior (corridors always connect with no change to the corridor carve).
- **Expansion-to-touch**: rooms grow toward their *linked* edges (only where a corridor exists), capped
  one cell short of the block boundary so a ≥2-cell doorway gap always remains (rooms never merge). At
  `liminality 0` connected rooms nearly touch — floor jumps ~1528→~4996 tiles, corridors become short
  doorway gaps, the dead negative space collapses.
- **Bent corridors** (planned) were *skipped*: expansion already shrinks corridors to doorway gaps, so
  bending them adds negligible value at high carve-rewrite risk.

## Correctness bugs fixed (from the max-effort review)
- gore blood decals no longer float above the knee-high camera-facing walls (added the
  `is_camera_facing`/`SHORT_CAMERA_WALLS` filter the other subsystems already apply).
- Deleted the dead `room_cutaway` "backup mode" — removed `occlusion.rs`, `OcclusionPlugin`,
  `WallMaterials`, and `Dungeon::region_at`; knee-walls are the single camera-occlusion path (one-path).

## Verification
- `cargo test` — 47 passing (8 new: 6 dungeon-generation, 2 furniture-coupling, plus 2 wfc boundary).
- `cargo run` boots clean: `crab: surface graph built — 2189 patches (1528 floor, 661 wall)`,
  `spawned 40 crabs across 4 nests`, no panic. devshot screenshot confirms the Backrooms look at
  `liminality 1.0` and the office/desk coupling.

## References
- Karth & Smith 2017, "WaveFunctionCollapse is Constraint Solving in the Wild" (10.1145/3102071.3110566).
- Merrell, Schkufza & Koltun, "Computer-Generated Residential Building Layouts" (per-room area+aspect).
- Smelik et al. survey (10.1111/cgf.12276). Plan: `~/.claude/plans/make-a-plan-to-composed-bentley.md`.
