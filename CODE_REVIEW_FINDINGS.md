# Code Review — Findings & Resolution (2026-07-05)

Max-effort multi-agent review over the full committed codebase. 15 findings verified; **13 fixed, 1
deferred, 1 false positive.** All fixes applied to the working tree (not committed). **`cargo check`
passes clean in both `dev` and `release`, zero warnings.**

Fixes are isolated from the in-flight `placement/`/`rng.rs` WIP — the only shared files (`main.rs`,
`crab.rs`, `squad.rs`) were edited far from your render-scale/placement changes.

---

## Correctness — fixed

| # | Site | Fix |
|---|------|-----|
| [0] | `crab.rs` `assign_meat_targets` | Clear `hauling` alongside `target` when a hauled gib is despawned (cap-recycled) — ends the permanent `Carry` soft-lock; the crew now re-forages instead of freezing. |
| [1] | `gore.rs` `read_settings` | Validate `autogib_min_pieces <= autogib_max_pieces` at load and **fail loud** — no more `i32::clamp` panic mid-combat on inverted config. |
| [2] | `gore.rs` `drain_gore` | Reordered: permanent **meat chunks + fragment gibs spawn for every kill regardless of fog**; only the visual feedback (blood, shake, decals) stays LOS-gated. Off-screen kills feed the crab economy again. |
| [3] | `ai/tuning.rs`, `gore.rs`, `vhs.rs` | Malformed config now **fails loud** (`error!` + `exit(1)`) instead of silently running on defaults. Absent (optional) file still uses defaults; present-but-unreadable also fails loud. |
| [5] | `gore.rs` `spawn_meat_chunks` | Clamp meat carry-weight to `[FRAG_WEIGHT_MIN, FRAG_WEIGHT_MAX]` like fragments — an un-loaded volume fallback can no longer make a chunk un-liftable. |
| [6] | `gore.rs` `confine_gibs` | Skip wall-confinement for chunks in `Hauling` phase (Kinematic, driven by `carry_gibs`); keep `prev` synced so confinement resumes cleanly if dropped. No more haul/​confine tug-of-war at wall corners. |
| [7] | `squad.rs` `recolor_units` | Mint the `StandardMaterial` **lazily** (only when a recolorable mesh is found) — no more one-orphaned-material-per-unit-per-frame during async GLB load. |
| [4] | `main.rs` | Gate `mod devshot;` **and** its plugin registration behind `#[cfg(debug_assertions)]` (restructured `main()` to `let mut app`). Release builds no longer ship the dev screenshot module — verified by a clean `cargo check --release`. |
| [8] | `combat.rs`, `enemies.rs` | **Deleted** both dead placeholder files (never `mod`-declared; imported a nonexistent `crate::player`). Refreshed the stale "files kept, not compiled" note in `main.rs`. Recoverable via git if you want them back for the future combat system. |

## Cleanup — fixed

| # | Site | Fix |
|---|------|-----|
| [11] | `util.rs` + `gore`/`autogib`/`blood_lens`/`audio` | Moved `hash_f32` into `util.rs` (byte-identical across 3 copies) and pointed `audio.rs` at the existing `util::{next_u32, rand01}`. One home for the hand-rolled RNG/hash surface. |
| [12] | `autogib.rs` | Extracted `triangle_indices(mesh, vertex_count)` — replaces the two near-verbatim index-decode blocks in `mesh_signed_volume` and `append_mesh`. |
| [13] | `autogib.rs` | Removed the single-variant `enum BakePose { Static }` and its one-arm match (scaffolding for an unbuilt skinned path); fixed the dangling `[BakePose]` doc link. |
| [14] | `impact_fx.rs` | Corrected the module doc — it claimed a live egui Save/Load panel that doesn't exist; it loads the RON once at startup. |

---

## Not applied

**[9] — deferred (shader `hash21`/`rand_dir` dedup across 4 WGSL files).** Real, but purely cosmetic.
The fix (Bevy `#import` of a shared WGSL library) is a runtime-shader-compilation change that `cargo
check` can't validate — it needs the game launched and a frame rendered to confirm the shaders still
compile. Not safe to do blind while the render/placement WIP is live. Worth a focused follow-up where
I launch the game (via `devshot`) to verify.

**[10] — false positive (remove `rand`/`bevy_rand`/`rayon`).** These are the placement-grammar WIP's
dependencies (`rand` in `rng.rs`, `rayon` reserved for Stage-3 MCMC). Left as-is. The review scoped to
committed code, which predated the WIP.

---

## Refuted during review (verified NOT bugs)

- `enemy.rs` double-despawn — no panic in bevy 0.19 (`Commands::despawn` on a dead entity is a no-op).
- `dungeon.rs` `min_by` `unwrap` — operands are integer-derived finite `f32`, never NaN.

---

## Verification

- `cargo check` (dev): clean, 0 warnings.
- `cargo check --release`: clean (confirms devshot stripped).
- Behavioral fixes ([0], [2], [5], [6]) are logic-level and compile-clean; **not** exercised at runtime
  — they need sustained combat to observe, and the game wasn't launched to avoid disrupting the active
  WIP session. Drive them in-game when convenient.
