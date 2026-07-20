# Backlog

Deferred work that is understood and scoped but intentionally not done yet — with enough context to pick
up cold. Remove an item when it lands (reference the PR).

---

## Split `src/dungeon.rs` (~3040 lines) into a `dungeon/` submodule

**Origin:** 2026-07-19 code review, "large files" finding. Its sibling — splitting `crab.rs` (2643 L) into
`crab/{mod,setup,movement,combat,foraging}` — shipped in PR #62; **this half was deferred.**

**Why deferred (not just skipped):**
1. `crab.rs`'s systems were top-level `fn`s, so a scripted contiguous-range move + `pub(crate)` widening
   split cleanly (2 trivial field-visibility fixes, goldens unchanged). `dungeon.rs`'s query/collision API
   (`is_solid`, `footprint_on_floor`, `footprint_clears_openings`, `wall_faces_near`, `resolve_move`,
   `line_of_sight`, `cell_center`, …) lives **inside one `impl Dungeon { … }` block** alongside `generate`.
   Splitting generation from geometry means closing that `impl` and reopening a second `impl Dungeon` in the
   other file — mid-`impl` surgery the range-move script can't do safely.
2. At defer time `dungeon.rs` also carried **pre-existing uncommitted WIP** (not from the review session),
   so a botched split wasn't cleanly revertible. Do this split only from a clean `dungeon.rs`.

**Proposed shape** (follow the `squad_ai/` and new `crab/` pattern):
- `dungeon/mod.rs` — module doc, imports, constants, the `Dungeon`/`Tile`/`Wall`/`FloorMaterials` structs,
  `DungeonPlugin`, `mod` decls + re-exports, and the `#[cfg(test)] mod tests` (keep
  `golden_dungeon_snapshot_is_stable`).
- `dungeon/config.rs` — `DungeonConfig`/`Topology`/`WfcWeights`/`RoomType`/`NotchConfig` + `parse`/`validate`.
- `dungeon/cutaway.rs` — the view-relative `Cutaway*` components + `update_cutaway` (cosmetic; cleanest seam).
- `dungeon/generation.rs` — coarse + graph layout, `carve`, `generate`, and the free-fn gen helpers.
- `dungeon/geometry.rs` — the query/collision `impl Dungeon` methods (a *second* `impl Dungeon` block).
- `dungeon/spawn.rs` — `wall_mesh` + the `spawn_tiles` system.

**Approach that worked for `crab.rs`** (reuse it): `pub(crate) use` the external imports in `mod.rs` so
submodules need only `use super::*`; widen moved private items to `pub(crate)`; expect a few struct-field
`pub(crate)` fixes (E0451) for types accessed across the new boundary.

**Acceptance:** pure code move — **determinism-neutral**. `cargo test` + `cargo test --features test-harness`
(esp. `golden_dungeon_snapshot_is_stable` and the replay `GOLDEN`/`GOLDEN_FIELD`) must be **unchanged**;
`cargo check --release` clean, zero warnings. If any golden moves, an unintended logic change slipped in —
investigate, don't re-pin.
