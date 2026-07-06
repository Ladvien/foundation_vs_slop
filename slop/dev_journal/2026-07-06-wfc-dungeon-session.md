# Dev Journal: 2026-07-06 — WFC dungeon: review → RON config → realistic rooms → liminality dial

**Session Duration:** ~one long session (review + Phases 1–2 + 2 bug fixes + Phase 3 plan)
**Walkthrough:** None
**Companion doc:** `2026-07-06-dungeon-config-and-realistic-rooms.md` (technical summary); this entry is the
reflective narrative. Phase-3 roadmap: `slop/research/2026-07-06-phase3-graph-wfc-plan.md`.

## What We Did

Started from a max-effort `/code-review` of the WFC room/hallway generator, cross-referenced against the
home-still research corpus (Karth & Smith 2017 "WFC is Constraint Solving in the Wild"; Kim et al. 2020
graph-WFC; Merrell et al. residential layouts; Tutenel/Smelik survey; Panero anthropometrics). The review
found the WFC itself was sound but three design gaps (nothing RON-configurable, unrealistic uniform room
sizes, a rigid grid-of-boxes) plus correctness issues.

Then implemented, on branch `dungeon-wfc-improvements` (6 commits, all tested + verified in-game):
- **Phase 1** — `assets/dungeon.ron` + `DungeonConfig` (required, fail-loud, validated); type-driven
  realistic room sizing (per-room area + aspect, Merrell 2011); furniture couples to the generated room
  type via `Region.props.tags` (dropping the `region_id % 4` placeholder); a WFC boundary-invariant fix;
  and no-panic conversions.
- **Two confirmed review bugs** — gore blood decals floating above knee-walls; a dead `room_cutaway`
  "backup mode" (deleted the whole `occlusion.rs` chain).
- **Phase 2** — the `liminality` dial: room jitter + expansion-to-touch, branch-free `(1-liminality)`
  scaling. At `liminality 0.0` floor area went 1528 → 4996 tiles (rooms grow toward links, dead space
  collapses); at `1.0` it's byte-identical to before.
- Wrote the **Phase 3** (graph-WFC, hand-rolled) execution plan.

## Bugs & Challenges

### WFC boundary invariant: corridors dead-ending into the void

**Symptom:** The `wfc.rs` module doc promised "a Link always meets a Link, corridors never dead-end into
rock," but nothing enforced it at the grid edge.

**Root Cause:** Propagation skips off-grid neighbours (`wfc.rs`, the `nx<0||...continue` guard), so a
boundary cell could collapse to a prototype whose *off-grid* edge is a Link — a corridor pointing at
nothing. Connectivity/boundary are **global** constraints local propagation can't see (Karth & Smith).

**Solution:** Added a required `initial: &[u32]` per-cell starting-domain param to `collapse_grid` (a CSP
*unary* constraint). `boundary_initial` clears, from each edge cell, every prototype with an open off-grid
edge. Rock always survives, so it can never itself contradict. Made it one-path (a required slice, not an
`Option` with a `None` branch) and extracted `propagate` into a shared helper so the initial pass and
every collapse step use one code path.

**Lesson:** "Fix the invariant at the door" beats rejection-sampling. An initial-domain constraint that
*can't* contradict avoids a retry storm.

### Zero-room collapse → startup panic

**Symptom:** `.expect("dungeon must contain at least one room")` — a latent crash the review flagged as
exactly what RON-adjustable small grids would expose.

**Root Cause:** An all-Solid collapse is a *valid* WFC result (rock is self-compatible), not a
contradiction, so `wfc::generate` never re-rolls it; `largest_room_component` then returns empty.

**Solution:** `generate` returns `Result`; the whole coarse collapse retries with offset seeds until it
yields ≥1 room, then fails loud. `partial_cmp().unwrap()` → `f32::total_cmp`. Crucially, attempt 0 uses
the seed unchanged, so a config that already produces rooms is byte-identical.

**Lesson:** Distinguish "contradiction" (no solution) from "valid but degenerate" (a solution you don't
want). They need different handling.

### The dead `room_cutaway` "backup mode" and its deletion cascade

**Symptom:** Two full camera-wall-hiding implementations coexisted (knee-walls vs `room_cutaway`), gated
by a const — a "backup mode" the one-path rule forbids. `room_cutaway` early-returned under the live
knee-wall config, so it was pure dead code.

**Investigation → cascade:** Deleting `room_cutaway` orphaned `OcclusionPlugin` → then the `WallMaterials`
resource (only `room_cutaway` read it) → then `region_at` (its only caller) → then the `RegionId` import.
Each `cargo build` surfaced the next unused item.

**Solution:** Followed the cascade to completion — deleted `occlusion.rs` entirely, removed the plugin
registration + module decl, dropped the transparent material, and updated the doc comments (no more
"revert to 1.0"). Committed to knee-walls as the single path. Verified behavior-neutral in-game.

**Lesson:** Deleting a dead branch cleanly means chasing its whole dependency tail. Let the compiler's
dead-code warnings drive the cascade one hop at a time.

### Determinism across the dial (keeping `liminality 1.0` byte-identical)

**Challenge:** Adding jitter/expansion must not change the shipped Backrooms layout.

**Solution:** Every perturbation is `base + delta·(1 - liminality)`, and — key detail — `jitter_origin`
returns *before drawing any RNG* when `t == 0`. So at `liminality 1.0` the RNG stream is untouched and
the layout is bit-for-bit the old one. Expansion draws no RNG at all (pure function of `t` + links).

**Lesson:** "Superset" isn't just `delta·0 = 0` in the formula — you must also not *consume RNG* on the
no-op path, or the downstream stream drifts and everything after diverges.

### Expansion-to-touch without merging rooms

**Challenge:** Grow rooms toward links so they share walls, but rects must stay rectangular, rooms must
not overlap/merge, and corridors must still connect — all without rewriting the corridor carve.

**Root Cause (of the subtlety):** If two rooms' rects literally abut (`A.max == B.min`), their edge floor
cells are *adjacent* → no wall between them → they merge into one open space (walls live on floor↔rock
edges). And the doorway-neck would rock a cell inside the neighbour.

**Solution:** Cap each linked edge **one cell short of the block boundary**, guaranteeing a ≥2-cell rock
gap the existing corridor+neck bridges. Rooms only grow (never move the block centre off-interior), so
the *unchanged* center-to-center corridor still connects. Added a no-overlap safety-net test.

**Lesson:** On a tile grid, "walls" are edges, not cells — "touching" rooms merge. Keep a rock gap and
let the doorway punch it.

### The const/config split (what can't move to RON)

**Challenge:** The review wanted generation params RON-configurable. But `TILE_SIZE`/`WALL_THICKNESS`/
`WALL_HEIGHT` are consumed by `const` initializers in *other* modules (`squad.rs`, `metropolis.rs`,
`nest.rs`) — a `const` can't be initialized from a runtime value.

**Solution:** Grep-verified that only generation params (`COARSE_*`, `BLOCK`, `ROOM_*`, corridor, weights)
have zero cross-module use; moved exactly those to `DungeonConfig`. Physical wall dims stayed compile-time
`const`. No value lives in both — no dual path.

**Lesson:** "Make it configurable" has a hard boundary at `const`-context consumers. Analyse consumers
before deciding what's a runtime knob.

### The review workflow reviewed the *old* code

**Symptom:** The max-effort review (launched at the start, ran ~64 min) reported bugs I'd already fixed.

**Solution:** Triaged all 14 findings against the working tree — 4 already fixed by Phase 1, 1 was exactly
Phase 2's job, the rest were new pre-existing bugs. Fixed the two the user picked; reported the rest.

**Lesson:** A long-running background review races your own edits. Map its findings to current state
before acting on any.

## Code Changes Summary

- `src/wfc.rs`: data-driven prototype weights; `collapse_grid` `initial`-domain param + `boundary_initial`;
  shared `propagate` helper.
- `src/dungeon.rs`: `DungeonConfig`/`WfcWeights`/`RoomType` + `parse_config` validation; `generate` →
  `Result` with zero-room retry; `pick_room` type-driven sizing; `jitter_origin` + expansion-to-touch;
  panic-path fixes; deleted `WallMaterials`/`region_at`; test module (10 tests).
- `src/placement/furnish.rs` + `furniture.ron`: `room_profile` reads `region.props.tags`; office/bathroom
  furniture entries.
- `src/gore.rs`: knee-wall filter on blood-splatter placement.
- `src/main.rs` + **deleted** `src/occlusion.rs`: removed the dead cutaway path.
- `src/rng.rs`: removed now-unused `range_usize`.
- `assets/dungeon.ron` (new): the generation config.

## Patterns Learned

- **Branch-free superset scaling** (`base + delta·(1-t)`, *and* no RNG on the no-op path): add a dial that
  provably reduces to the old behavior at one extreme.
- **Initial-domain CSP constraint**: enforce a "global" boundary rule as a per-cell unary constraint that
  can't contradict — cleaner and safer than post-hoc rejection.
- **Config-selected routing ≠ fallback**: a `Topology`/mode enum chosen from config is one-path; a
  silent alternative on failure is the forbidden second path.
- **Consumer analysis before "make it configurable"**: `const`-context consumers pin a value to
  compile-time.
- **Let dead-code warnings drive a deletion cascade** one hop at a time.
- **Devshot + boot-log as a layout signal**: the squad-follow camera can't show the whole map, but the
  crab `surface graph built — N patches` line quantifies floor area (1528 → 4996 proved expansion worked).

## Open Questions

- Phase 3's variable-degree prototype representation vs the `u32` mask cap (degree ≤5 by pruning Delaunay,
  or widen the domain?) — flagged in the plan for the implementer.
- Whether to keep `SHORT_CAMERA_WALLS` as an always-true flag or inline it (left as-is; low value, high
  churn).
- Several reported-not-fixed pre-existing bugs (resolve_move NaN, wall_mesh silent-UV, is_camera_facing
  float-`==`, `unreachable!()` arms) await a separate pass.

## Next Session

**Phase 3: graph-based WFC (hand-rolled).** Full roadmap in
`slop/research/2026-07-06-phase3-graph-wfc-plan.md`. Step 1 is the behavior-neutral refactor of the carve
into `CoarseLayout` + `expand_to_fine` (the correctness gate), then `collapse_graph`, then Poisson-disk +
Bowyer–Watson Delaunay, then the `Topology::Graph` route. Branch `dungeon-wfc-improvements` (6 commits,
un-pushed). Status memory: `dungeon-wfc-improvements-status.md`.
