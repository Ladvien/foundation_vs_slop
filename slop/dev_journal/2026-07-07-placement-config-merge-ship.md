# Dev Journal: 2026-07-07 — Placement config merge & ship (PR #1)

**Session Duration:** ~1 hour (ship phase only)
**Walkthrough:** None — this was the land-it phase for the placement grammar Phase 3 work
(contextual furniture rules + nested-grid on-surface stacking) built in earlier sessions.

## What We Did

Took the `worktree-nested-grid-placement` branch from "draft PR, green tests locally" all the way
to **merged on `main`**:

1. Freed a **100%-full disk** that was blocking builds (cleared the worktree's 17 GB `target/`).
2. Marked draft **PR #1** ready for review — which immediately surfaced a `CONFLICTING` state
   because `main` had advanced by 3 commits while the branch was in flight.
3. Discovered the conflict was a **structural refactor collision**: `main` had unified all the
   standalone `assets/*.ron` config files (including the placement manifest + Metropolis weights)
   into a single `assets/config/config.ron` loaded via `crate::config::GameConfig`. My branch still
   authored the now-deleted `assets/placement/{furniture,metropolis}.ron`.
4. **Resolved the merge** by migrating my data changes into the new unified config and repointing
   code that referenced the deleted files (details below).
5. Verified: full lib rebuild (clean, exit 0) + `cargo test` deterministic-core gate — **116 tests
   passed, 0 failed** (107 lib + 6 rng_guard + 3 wfc_pin).
6. Pushed the merge, PR went `MERGEABLE`, user merged it (squash `c41bbf9`).
7. Fast-forwarded the primary checkout's `main` (`97c1c04 → c41bbf9`) and tore down the worktree.

## Bugs & Challenges

### 1. Disk 100% full mid-build

**Symptom:** A trivial `gh pr ready` invocation failed with `ENOSPC` — the harness couldn't even
write the command's output file.

**Investigation:** `df -h` showed only 245 MiB free on the data volume; the worktree's `target/`
alone was 17 GB (multiple full + `--features test-harness` builds across sessions/worktrees).

**Root Cause:** Build-artifact accumulation across parallel worktrees on a nearly-full 460 GB disk.

**Solution:** `rm -rf target/` in the worktree (work was already committed + pushed, so artifacts
were disposable). Freed 17 GB → 96% used, builds resumed.

**Lesson:** On a shared, near-full disk, `target/` dirs across worktrees are the first thing to
reclaim. They're always rebuildable; never precious once the branch is pushed. Cost paid later: the
next build was a full from-scratch recompile (~3–4 min) that blew the default 2-min Bash timeout —
run long rebuilds with `run_in_background: true`.

### 2. Merge conflict from a config-system refactor on `main`

**Symptom:** PR flipped to `mergeable=CONFLICTING, state=DIRTY` the moment it left draft.

**Initial Hypothesis:** Simple textual overlap in a shared file (README or furniture.ron).

**Investigation:** `git merge origin/main` reported **modify/delete** conflicts on
`assets/placement/furniture.ron` and `metropolis.ron` (deleted on `main`, modified on my branch)
plus a content conflict in `src/dungeon.rs`. `grep`-ing `src/config.rs` revealed the new shape:
a `PlacementConfig { furniture: FurnitureManifest, metropolis: MetropolisWeights }` deserialized as
a `placement:` slice of one master `config.ron`.

**Root Cause:** Two branches evolved the same data along orthogonal axes — mine changed the
*content* of the placement RON (new Scatter roles, `height`, `back_to_wall`, `group`), `main`
changed its *location/loading mechanism* (standalone files → unified config). Git can't auto-merge
"file deleted here, edited there."

**Solution:** Manual migration, one path preserved (no fallback, per house rules):
- Transplanted every furniture change into `config.ron`'s `placement.furniture.items` (re-indented
  to the nested slice) and added the three new weights (`w_hard`, `w_wall_angle`, `w_group`) to
  `placement.metropolis`.
- `git rm`'d the two deleted `assets/placement/*.ron`.
- Took `main`'s `load_game_config()`-based version of the `dungeon.rs` topology-default test (my
  side only differed because my branch-point predated the refactor; my commits never touched it).

**Lesson:** When a PR conflicts right after leaving draft, check whether `main` did a *structural*
refactor, not just a line edit. Diagnose with the actual `git merge` (modify/delete vs content),
and read the *new* module (`config.rs`) to learn the target shape before hand-porting. A pure
data-vs-mechanism split always needs manual migration.

### 3. Integration test still loading deleted files

**Symptom:** After resolving the RON conflicts, the `furnish_region` end-to-end test in
`furnish.rs` still hard-referenced `assets/placement/furniture.ron` / `metropolis.ron`.

**Root Cause:** The test loaded the shipped manifest + weights directly from the old file paths —
which no longer exist post-refactor.

**Solution:** Repointed it to `crate::config::load_game_config()` and pulled
`cfg.placement.furniture.clone()` / `cfg.placement.metropolis.clone()`, dropping the now-unused
`load_manifest` / `MetropolisWeights` imports. This also makes the test exercise the *real* shipped
config path end-to-end, which is strictly better than reading loose files.

**Lesson:** After a config-loading refactor, grep tests for the old asset paths — they don't fail
to compile (string literals), they fail at runtime with a file-not-found, so they're easy to miss
until the suite runs.

## Code Changes Summary (the merge commit `4ee697f`, squashed into `c41bbf9`)

- `assets/config/config.ron`: migrated placement data into the unified `placement:` slice — Scatter
  roles + `height` for plant/lamp/TV, `back_to_wall` for fridge/bath fixtures, `group: "bath"` for
  toilet+sink, and three new Metropolis weights (`w_hard` 12.0, `w_wall_angle` 1.0, `w_group` 1.5).
- `assets/placement/{furniture,metropolis}.ron`: **deleted** (superseded by the unified config).
- `src/placement/furnish.rs`: repointed the end-to-end test to `load_game_config()`.
- `src/dungeon.rs`: took `main`'s config-loader version of the topology-default test.
- (auto-merged with `main`'s refactor) `manifest.rs` group/height fields + split-out
  `validate_manifest`, `metropolis.rs` Both-hardness + hard/wall-angle/group terms,
  `scatter.rs` new module, `ir.rs`/`solver.rs` Scatter plumbing.

## Patterns Learned

- **Data-vs-mechanism merge migration**: when your branch changed a config file's *contents* and
  `main` moved *where/how it's loaded*, resolve by hand-porting the content into the new home and
  deleting the old file — don't try to keep both (violates one-path).
- **Dispose of `target/` under disk pressure**: pushed branch ⇒ build artifacts are free to delete.
- **Verify the migration end-to-end, not just the compile**: repointing a test to the real config
  loader proved the whole `config.ron → GameConfig → furnish` path, which is what actually shipped.

## Open Questions

- None blocking. The GPU/harness CI lane (`replay + liveness`) was independently fixed on `main` in
  PR #5 (stopped pinning the lavapipe ICD path), so the headless-GPU tests are green now too — worth
  a local `cargo test --features test-harness -- --test-threads=1` run next session to confirm the
  merged placement code passes the liveness/SSIM oracles, not just the pure-CPU gate.

## Next Session

- Run the headless harness suite against merged `main` to confirm placement changes hold up under
  the liveness/SSIM oracles (the pure-CPU gate passed; the GPU gate wasn't run locally this session).
- Placement grammar Phase 4+ (per the placement-grammar-status memory) if continuing that track.
