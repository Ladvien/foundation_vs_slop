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

**Two notes to add to the plan before executing** (2026-07-20 code review, L11):
1. **Move order matters.** Extract in this order: `cutaway.rs` (top of file) → `config.rs` → `geometry.rs`
   → `spawn.rs`, leaving `generation.rs` as the residual. Doing `generation` before `geometry` requires
   moving code *around* the `impl Dungeon` block — the fragile case the plan wants to avoid (generation's
   free fns are interleaved with the `impl` block, lines ~522–1170 + 1665–1850).
2. **Test-constructor methods** (`test_*_dungeon`, `from_walkable_mask`, `from_test_layout`, ~lines
   1340–1450) live inside the *production* `impl Dungeon` block, not in `#[cfg(test)]`. After the split they
   land in `geometry.rs`. Either move them into a `#[cfg(test)] impl Dungeon` block in `mod.rs` (cleaner)
   or leave them in `geometry.rs` with a `// test-only` comment (matches current state). Also: keep the
   810-line test module (2233–3044, 43 test fns) in `mod.rs` — it crosses every submodule boundary and uses
   `pub(crate)` internals from all of them. Splitting tests by submodule would force widening many items
   across the new boundary for no cohesion win.

---

## `#[ignore]` or delete `zz_localize_g0` temp debug probe

**Origin:** 2026-07-20 code review, H7 (`tests/replay.rs:636-700`).

**Why deferred:** Trivial cleanup, but it's in the test file and the comment says "Remove once the tie-break
is found" — the tie-break *was* found (G0c, `GibKey`), so this is now dead weight.

**The issue:** `zz_localize_g0` is not `#[ignore]`d, so it runs on every harness invocation. It does 24 ×
7200-tick traces under load (~minutes), and **only `println!`s — it never asserts**. Pure dead weight in
the harness lane. The `zz_` prefix was presumably to sort it last; it doesn't prevent execution.

**Proposed fix:** Either `#[ignore]` it (preserves the probe for a future G-class hunt) or delete the whole
function. Deleting is cleaner — the G0 hunt is documented in `TESTING.md` and `docs/rl/2026-07-16-search-
rollout-nondeterminism.md`; the probe is not the reusable part.

**Acceptance:** `cargo test --features test-harness` runtime drops by ~minutes; no test count change if
`#[ignore]`d, one fewer test if deleted.

---

## Reconcile Smiley observation: player camera vs squad-member LoS

**Origin:** 2026-07-20 code review, C1 (`src/enemy.rs:1041-1119` vs `README.md:36-37`).

**Why deferred:** This is a **design decision the user must make** — the code and the README disagree, and
either could be authoritative. Do not implement until the user decides which.

**The disagreement:** README says: *"If the Smiley is in LoS of any squad member and its attacked, it looks
scared and runs away. But if no squad member is looking directly at it (raytracing area), and its attacked,
it looks angry..."*. The code at `enemy.rs:1041-1119` redefines "observed" as **the player's camera
view-cone centering it** (`is_watcher_centered` dot ≥ `gaze_cos`). The long comment at `enemy.rs:104-119`
admits this is a deliberate rewrite: *"The OLD gaze keyed on whichever squad figurine's auto-aim cone
happened to point at it — invisible and arbitrary to the player; keying on the camera makes 'if I watch it,
it hides' literally true and player-controlled."*

**Two behavioural consequences the README does not authorise:**
- A squad member standing right next to the Smiley staring at it does **not** scare it if the player's
  camera is looking elsewhere.
- The Smiley can unleash on a unit that **is** looking at it (the squad's auto-aim cone is irrelevant), as
  long as the human at the camera isn't centering it.

**Ask the user before implementing:**
1. **Assume code is correct → update README** to describe the camera-gaze model. Small doc change, no
   gameplay change, no golden movement. Also: re-ground `ZAP_RANGE ≤ LOOK_RANGE` (M7, `enemy.rs:122-128`)
   on the new observation source — the test at `enemy.rs:1384-1386` pins an invariant that's stale under
   camera-gaze (gaze range is now infinite, ZAP is bounded by `VISION_RADIUS`).
2. **Assume README is authoritative → restore squad-member LoS raytrace.** Gameplay change; moves goldens;
   needs re-pinning. The `psi_vision`/fog LOS infrastructure already exists for this.

**Acceptance:** README and code agree, either way. If option 1, the `ZAP_RANGE` invariant is re-grounded or
the test is updated. If option 2, the deterministic-core golden moves *deliberately* and is re-pinned with
a human-reviewed diff.

---

## Reconcile crab "numbers-kill" >5 gate removal

**Origin:** 2026-07-20 code review, C2 (`src/crab/combat.rs:91-115` vs `README.md:29`).

**Why deferred:** Design decision the user must make — the code and the README disagree.

**The disagreement:** README says: *"Under ~5 crabs on a target, zero damage. Past that the bite scales
super-linearly — a pile shreds a unit in seconds."* The code at `combat.rs:109-110` applies
`crab_contact_dps * count^exponent * dt` with **no `MASS_MIN` gate** — a single crab deals nonzero damage
(`2.3 * 1^1.5 = 2.3` DPS). The comment at `combat.rs:60-65` documents this as a deliberate change: *"The
old hard `MASS_MIN` gate made 1–4 crabs deal literally zero damage, so a thinned/split swarm played
harmless; the super-linear curve already makes a lone crab weak and a pile terrifying without a dead zone."*

**Ask the user before implementing:**
1. **Assume code is correct → update README** to remove the "zero damage under ~5" claim. No gameplay
   change.
2. **Assume README is authoritative → restore the `MASS_MIN` gate** (e.g. `if count < 5 { return; }` before
   the damage line). Gameplay change; moves goldens; needs re-pinning. Also restore the pounce
   critical-mass gate (C3, below) for consistency.

**Acceptance:** README and code agree. If option 2, the deterministic-core golden moves *deliberately*.

---

## Reconcile pounce critical-mass gate removal

**Origin:** 2026-07-20 code review, C3 (`src/crab/movement.rs:721-745` vs `README.md:30`).

**Why deferred:** Design decision the user must make — paired with C2 above.

**The disagreement:** README says: *"Pounce. Near a unit, hunker then leap a ballistic arc (~10 body
lengths), biting on landing. Same critical-mass rule: a lone leaper lands but does no damage."* The pounce
landing at `movement.rs:735-745` always bites for the full `crab_jump_damage` if the nearest prey is in
`reach_sq`. The comment at `movement.rs:725-727` says: *"No critical-mass gate: the old `MASS_MIN` check
made a lone leap deal zero, so a pouncing crab read as a harmless hop; a lunge that connects should hurt."*

**Ask the user before implementing:**
1. **Assume code is correct → update README** to remove the "a lone leaper lands but does no damage" claim.
   No gameplay change.
2. **Assume README is authoritative → restore the critical-mass gate on pounce landing.** Gameplay change;
   moves goldens; needs re-pinning. Do this together with C2 — the "numbers kill" identity is one design
   choice, not two.

**Acceptance:** README and code agree. If option 2, the deterministic-core golden moves *deliberately*.

---

## Promote the harness lane from `continue-on-error` to a hard CI gate

**Origin:** 2026-07-20 code review, H2 (`.github/workflows/ci.yml:67`).

**Why deferred:** The harness is GPU-free now (`RenderPlugin { backends: None }`), so the *technical* reason
for `continue-on-error` is gone. But the harness lane is slow (~6 min for the G0 guard, ~40 min for the
mutant guard), so promoting it changes CI wall-time meaningfully. Worth doing with a timeout strategy.

**The issue:** The entire headless harness lane — `replay.rs` (golden lock + G0 guard + mutant guard),
`liveness.rs`, `search_calibration.rs`, `search_parallel.rs`, `playtest_level.rs` — **cannot fail a build**.
The tests that caught G0/G0b/G0c are decoration on CI. TESTING.md itself says this lane "is a candidate for
promotion to a hard gate." The release-determinism job only runs `cargo test --release` (the easy half —
the deterministic core never enters the racy paths per invariant 9).

**Proposed:**
1. Drop `continue-on-error: true` from the `harness` job.
2. Split the lane: a fast hard-gate job (`replay` minus the mutant guard + `liveness` + `search_calibration`
   + `playtest_level`, ~10 min) and a slow advisory job (the mutant guard, ~40 min, stays
   `continue-on-error` or moves to a nightly schedule).
3. Add a job timeout (e.g. 30 min for the fast lane) so a hung runner fails loudly instead of silently
   approaching the 6-hour limit.

**Acceptance:** A harness regression (e.g. reintroducing the `smiley_defense` cull bug) reds CI. A slow
run yellows (timeout), not greens.

---

## Add a regression test that baking the authored *levels* elite is byte-identical

**Origin:** 2026-07-20 code review, M14 (`src/squad_ai/level_genome.rs:186-187`).

**Why deferred:** Probably fine in practice — the f32-rounding is documented. But the "authored elite bakes
to a no-op" property is *not* pinned for `levels`, and the existing byte-identical-no-op test
(`splicing_the_shipped_config_with_its_own_values_is_a_byte_identical_no_op`,
`src/bin/train.rs:2387+`) covers other dims but not `dungeon`/`mycelia` via the level genome's f32-rounded
path.

**The issue:** WFC and graph weights are `f64` in `DungeonConfig` but stored as `f32` in the genome.
`decode(authored)` matches the shipped config only to f32 precision (test at `level_genome.rs:492-524` uses
`1e-4` tolerance). If `train apply levels` bakes the *authored* elite, `config.ron`'s `wfc_weights`/
`graph_links` would shift by f32 rounding. The bake path (`splice_block`) compares by parsed value via
`scalar_eq`, which might or might not treat `1.2_f64 as f32 as f64` as equal to `1.2`. This is a latent
reproducibility hazard.

**Proposed:** Add a test that bakes the authored *levels* elite into a copy of `config.ron` and asserts
byte-identical output, matching the existing test for other dims. If it fails, the f32-rounding is real and
either the genome stores f64, or `scalar_eq` needs to treat `f32 as f64` as equal to the source `f64`.

**Acceptance:** The test passes (or, if it fails, the underlying f32-rounding is fixed and the test then
passes).

---

## Make clippy block on a denylist (`unwrap_used`/`expect_used`/`panic`)

**Origin:** 2026-07-20 code review, M15 (`.github/workflows/ci.yml:54`).

**Why deferred:** The repo has ~26 standing clippy lints and predates style enforcement, so a blanket
`-D warnings` would fail on untouched code. But a *denylist* of the lints CLAUDE.md explicitly forbids
(`unwrap`/`expect`/`panic`/`unsafe`) is a mechanical guard against drift, which the project rules require.

**The issue:** clippy is `continue-on-error`. No mechanical check that `unwrap`/`panic`/`unsafe` patterns
stay out of shipped code. The review found zero such patterns today (every `.unwrap()`/`.expect()` is in
`#[cfg(test)]`), but without a gate, a future commit could regress silently.

**Proposed:** Add a clippy step that blocks on `-D clippy::unwrap_used -D clippy::expect_used -D
clippy::panic -D unsafe-code` (or a curated subset), separate from the advisory full-clippy run. Run it
on the game binary only (not `tests/` or `test-harness`-gated code, where `unwrap`/`expect` are
acceptable).

**Acceptance:** A commit adding `.unwrap()` to shipped `src/` code reds CI. Existing code passes (verify
by running the denylist locally first).

---

## Add a macOS CI lane for ARM↔x86 f32 divergence

**Origin:** 2026-07-20 code review, M16 (`.github/workflows/ci.yml`).

**Why deferred:** The codebase already had to work around ARM↔x86 f32 divergence (the
`LightField::fold_fingerprint` arch-dependence workaround in `replay.rs`; the `field_passes_are_bit_identical`
golden was re-pinned specifically because an ARM value failed on x86 CI). Yet CI runs ubuntu-latest only.
TESTING.md explicitly notes "f32 gameplay math may diverge on other CPUs/compilers."

**The issue:** No CI on the platform where the developer runs the game (the home cloud includes Apple
Silicon and x86 hosts). The `field_passes_are_bit_identical` golden even has prose about the ARM-pinned
value failing on x86 — and the CI that catches that class is absent.

**Proposed:** Add a `macos-latest` (Apple Silicon) job that runs at least `cargo test` (the deterministic
core). If the core goldens diverge cross-arch, either (a) pin per-arch goldens with a tolerance, or (b)
move gameplay math to fixed-point (the TESTING.md "Treat other platforms with tolerance unless gameplay
math moves to fixed-point" guidance). Start with (a); (b) is a larger project.

**Acceptance:** A cross-arch f32 divergence in the deterministic core reds the macOS lane (or yellows with
a tolerance, if that's the chosen strategy).

---

## Wire `map_elites_cma_mae_loop` into `rl_search` or remove it

**Origin:** 2026-07-20 code review, M17 (`src/squad_ai/map_elites.rs:216-309`).

**Why deferred:** The CMA-MAE loop (Fontaine & Nikolaidis 2023, the documented "SOTA upgrade to CMA-ME") is
implemented and unit-tested but carries `#[allow(dead_code)]` and is not reachable from any `train`
subcommand. "Implemented but unreachable" is a path the search can never take — the one-path rule forbids
keeping two paths.

**The issue:** `rl_search`'s emitter selection still picks isotropic vs CMA-ME on `--cma`; the CMA-MAE loop
at `map_elites.rs:216-309` is never called. The unit tests at 354-398 exercise it on synthetic problems
and pass, but no `train` subcommand can select it.

**Proposed (pick one — ask before implementing):**
1. **Wire it** — add a `--cma-mae` flag to `RlSearchConfig`/`SearchArgs` that selects the CMA-MAE emitter.
   Small change; the loop is already correct.
2. **Remove it + its tests** — the one-path rule. Cleanest if CMA-MAE is not yet a priority.

**Acceptance:** Either `--cma-mae` works end-to-end (a short `train rl --cma-mae` run illuminates archive
cells), or the dead code is gone and `cargo test` is unchanged.

---

## Register `ShaderLibraryPlugin` in the harness (or document the omission)

**Origin:** 2026-07-20 code review, M5 (`src/sim_harness.rs:376-425`).

**Why deferred:** Works today — the harness runs with `RenderPlugin { backends: None }`, so no pipeline
specializes and the `#import foundation::noise` resolution never fires. But it's fragile: any future
change that causes a material pipeline to specialize will fail with a confusing "cannot find import" error
while the windowed game works.

**The issue:** `ShaderLibraryPlugin` (`src/lib.rs:159`) registers the `foundation::noise` WGSL import
library and is added in the windowed game only. The harness does **not** add it, yet the harness adds
`VhsPlugin`/`BloodLensPlugin`/`ImpactFxPlugin` (`sim_harness.rs:416,424`) whose shaders `#import
foundation::noise`.

**Proposed (pick one — ask before implementing):**
1. **Register `ShaderLibraryPlugin` in the harness too** — cheap, side-effect-free, gives parity. One line
   in `build_headless_app`.
2. **Add a one-line comment at `sim_harness.rs:~376`** noting *why* it's intentionally omitted (shaders
   never compile with `backends: None`). Documents the fragility without changing behavior.

**Acceptance:** Either the harness has parity, or the omission is documented. No test change.

---

## Add a lint/test that every non-exempt config field appears in a genome `BOUNDS`

**Origin:** 2026-07-20 code review, H3 (CLAUDE.md rule: *"Ensure every feature added is correctly included
in the RL/QD systems for evolving."*).

**Why deferred:** This is the single biggest structural gap in the test suite relative to the stated rules.
The rule is convention today, not mechanism. A new gameplay config field that *should* be evolved but isn't
wired into the genome would ship with no test failure.

**The issue:** The genome modules use a manual `pub const N: usize = 89` (`behavior_genome.rs:40`), `103`
(`world_genome.rs:42`), `15` (`audio_genome.rs:34`), `NeuralPolicy::WEIGHT_COUNT`
(`policy_genome.rs:36`). The `encode`/`decode` functions hand-spell each field push
(`behavior_genome.rs:161-271`), with a `debug_assert_eq!(v.len(), N)` at the end — **elided in the
`--release` `train` driver**. The protection is `debug_assert` + round-trip tests + `authored_has_n_knobs
_and_sits_inside_bounds`. These catch:
- A field added to `BehaviorTuning` but **not** to `encode`/`decode`/`BOUNDS`: **NOT caught** unless `N`
  is also bumped — the round-trip test round-trips the *existing* fields, so a new un-searched field is
  invisible.
- A field added to `encode` but `N` not bumped: **caught by `debug_assert_eq!(v.len(), N)`** — but only in
  debug builds. In `--release`, the genome silently has the wrong length; `decode`'s length check would
  then reject every committed archive.
- A field added to `encode`/`BOUNDS`/`N` but **never read by any system**: **NOT caught by any test.**
  This is the actual CLAUDE.md rule, and it's **convention, not mechanism**.

**Proposed:** A lint similar to `determinism_lint.rs` that reflects over each `*Tuning` struct's fields (or
hand-maintains an exempt list with `// EXEMPT: <reason>` comments) and asserts every non-exempt field
appears in the corresponding genome's `BOUNDS`. The codebase doesn't do reflection, so the realistic
version is the `// EXEMPT:` convention enforced by a scan. Start with `BehaviorTuning` and `SimTuning`
(the two biggest gameplay slices), then extend to `PerceptionTuning`, `AudioTuning`, `WorldTuning`.

**Acceptance:** A commit adding a gameplay field to `BehaviorTuning` without adding it to `BOUNDS` (and
without an `// EXEMPT:` comment) reds the lint. Existing fields pass (with `// EXEMPT:` comments added
where genuinely out of scope).

---

## Add `GoreSettings.autogib_*` to a genome (self-incriminating coverage gap)

**Origin:** 2026-07-20 code review, H4 (`src/gore.rs:400-409`).

**Why deferred:** The most important instance of the H3 gap. The repo's own incident log proves these
knobs are gameplay-affecting, yet no genome evolves them. Do this first, before the general H3 lint, as a
concrete proof-of-concept.

**The evidence:** `src/squad_ai/coevolve.rs:462-465` documents that swapping the squad-member mesh
(greybox → VALKYRIE) changed gib chunk count (23 vs handful), and each death's larger meat "magnet" drew
the swarm harder onto the living squad, tipping held-in seed `0xD00D` from 5/5 survival into a wipe,
forcing its retirement. Gib count is tuned by `autogib_*` (6 knobs: `autogib_pieces_base`,
`autogib_ref_extent`, `autogib_min_pieces`, `autogib_max_pieces`, `autogib_min_fraction`,
`autogib_speed_mult`), yet no genome evolves them. The codebase *knows* gore affects the ecosystem and
retired a held-in seed over it, but the search cannot adapt gore.

**Proposed:** Add the 6 `autogib_*` knobs to a genome — either `world_genome` (gore is a world property) or
a new `gore_genome` dim (if gore should be independently evolvable, like `levels`). Add `BOUNDS` with
feasible ranges derived from the shipped values ± a design-sensible band. Wire `encode`/`decode` to
`GoreSettings`. Add to the `coevolve` dim set if using the existing `world_genome`.

**Acceptance:** A short `train evolve3` run illuminates archive cells with varied gore knobs; the shipped
gore settings decode to `authored()`. `cargo test` unchanged; `cargo test --features test-harness` may
move the `authored_has_n_knobs` count — update the test deliberately.

---

## Document why each un-evolved gameplay knob is out of scope

**Origin:** 2026-07-20 code review, H4 remainder (the gaps beyond `GoreSettings`).

**Why deferred:** Not every config field *should* be evolved — some are structural calibration or
visual-only. But the exempt set is currently implicit. Make it explicit so a future contributor knows
which gaps are intentional and which are oversights.

**The un-evolved gameplay-affecting knobs identified:**
- **`MetropolisWeights`** (10+ knobs: `iterations`, `temp_start/end`, `translate_sigma`, `rotate_prob`,
  `w_overlap`, `w_bounds`, `w_wall`, `w_min_distance`, `w_facing`, `w_clearance`, `w_hard`, `w_wall_angle`,
  `w_group`) — `src/placement/solvers/metropolis.rs:45-70`. Only `coherence` is in the level genome
  (`level_genome.rs:252`). The rest affect furniture placement quality → cover and lines-of-sight →
  gameplay. A level search that cannot tune placement weights is searching a strictly coarser space than a
  designer authors.
- **`PerceptionTuning` sight thresholds** — `src/behavior_tuning.rs:38-56`. Of 13 fields, only `leash`
  and `squad_think_interval` are in `behavior_genome.rs`. The sight thresholds (`examine_sight`,
  `threat_sight`, `psi_sight`, `ward_sight`, plus their `_release` Schmitt partners, plus
  `wounded_frac`/`wounded_frac_release`) directly determine what the squad perceives and thus when it
  chooses each role work — the exact behaviour the search optimizes.
- **`CrabTuning` / `ParasiteSwarmTuning` cadence knobs** — `caste_cooldown`, `caste_flips_per_tick`,
  `scout_wander_interval`, `rally_deposit_cooldown`, `crew_timeout`, `max_commit_dist`, `leap_min`/
  `hunker`/`air`, `stalk_band`/`strength`, `huddle_size`, `settle_arrive`, `cohesion_strength`, burst
  choreography timings (`burst_convulse_secs`, `erupt_trauma`, etc.). Gameplay-affecting (they shape
  swarm choreography) but not in `behavior_genome.rs`.

**Proposed:** For each un-evolved knob, add an `// EXEMPT: <reason>` comment at the `BOUNDS` site (or a
side document) explaining why it's out of scope — e.g. "visual-only", "structural calibration", "searched
implicitly via X", "out of scope until Y". Knobs that are *not* exempt should be promoted to a genome
(file follow-up items).

**Acceptance:** Every gameplay-affecting config field is either in a genome `BOUNDS` or has an
`// EXEMPT:` comment with a reason. The H3 lint (above) enforces this going forward.

---

## Add unit tests for the untested large modules

**Origin:** 2026-07-20 code review, L9.

**Why deferred:** These modules are covered *end-to-end* via the deterministic-core golden, so a regression
is caught — but it moves the golden opaquely rather than naming the function. Unit tests would localize
the failure and make the golden's movement reviewable.

**The untested large modules:**
- **`src/squad.rs`** (932 lines, 12 fns) — squad movement / order / flow-field. Movement is pinned state
  (appears in `snapshot_hash`), so it's indirectly covered, but no unit test of flow-field computation,
  order resolution, or anchor logic.
- **`src/crab/foraging.rs`** (902 lines, 12 fns) — belief-modulated foraging. The forage gradient
  direction (seek heal / flee cyanide / anosmic) is exactly the kind of per-cell decision that should have
  a pure unit test.
- **`src/audio.rs`** (874 lines, 20 fns) — din/deposit pipeline. `replay::a_mutated_audio_config_changes
  _the_sim` proves the *knob* reaches gameplay, but nothing tests the deposit→evaporate→perception math
  directly.
- **`src/ai/field.rs`** (631 lines, 29 fns) — stigmergy field deposit/evaporate/diffuse/hotspot pipeline;
  the single most golden-sensitive module (it's what `field_hash` folds). `field_passes_are_bit_identical`
  covers it end-to-end, but no unit test of individual diffusion/evaporation math. A sign-flip in a
  diffusion kernel would move the golden with no localized failure.
- **`src/surface_nav.rs`** (575 lines, 21 fns) — flow-field nav. Same story as `squad.rs`.
- **`src/squad_ai/evaluate.rs`** (563 lines, 17 fns) — `rollout`/`trace_episode`/`run_episode` core that
  every search test calls. Heavily exercised *as infrastructure* but never asserted on directly (e.g. "a
  rollout with zero enemies produces zero combat decisions"). A bug in `trace` recording would silently
  corrupt every descriptor.

**Proposed:** Add `#[cfg(test)] mod tests` to each, following the `ai/utility.rs` pattern (seed in, assert
the exact output). For the field/diffusion math, pin a small fixed-grid golden. For `evaluate`, pin a
zero-enemy and one-enemy rollout trace. For `squad`/`surface_nav`, pin a small flow-field build + steer
vector.

**Acceptance:** Each module has at least one unit test that names a specific function and asserts a
specific value. A regression in that function reds the unit test, not just the opaque golden.

---

## Move pure-logic `squad_ai` tests out of `test-harness` gate into the hard gate

**Origin:** 2026-07-20 code review, L10 (`src/squad_ai/{cmaes,map_elites,poet}.rs`).

**Why deferred:** Conservative gating — the tests are marked `test-harness` but are pure (no App, no GPU,
no harness). The gating means CMA-ES convergence, MAP-Elites illumination, and POET open-endedness are
**NOT in the CI hard gate** — they only run in the `continue-on-error` harness lane. A regression in
`SepCmaEs::tell` would not block CI.

**The issue:** `src/squad_ai/cmaes.rs`, `map_elites.rs`, `poet.rs` tests are `test-harness`-gated per
TESTING.md, but their tests are pure (CMA-ES on a sphere, POET on a synthetic eval closure, MAP-Elites on
a synthetic QD problem) — no App, no GPU, no harness. They could run in the `cargo test` hard gate.

**Proposed:** Remove the `test-harness` gate from the pure-logic tests in these three modules. Keep the
gate only for tests that actually need `sim_harness` or `rollout`.

**Acceptance:** `cargo test` now runs CMA-ES/MAP-Elites/POET tests, and a `tell` regression reds the hard
gate. `cargo test --features test-harness` unchanged.

---

## Add a pinned test that injects `WatchedByPlayer(true)` and asserts the Smiley flees

**Origin:** 2026-07-20 code review, H5 (`src/enemy.rs:1002-1019`).

**Why deferred:** The `Scared` state (a third of the README mechanic — "looks scared and runs away") is
never exercised in the harness because `snapshot_player_gaze` is windowed-only. A regression that breaks
`Scared` entirely (wrong flee direction, infinite flee) would not be caught by the liveness oracles.

**The issue:** `snapshot_player_gaze` is registered only in `lib::run` (`src/lib.rs:296`), not in
`sim_harness`. The deterministic harness reads `WatchedByPlayer(false)` forever, so the reflex can only
flip between `Watching` and `Unleashing`. The comment at `enemy.rs:984-991` claims this "keeps its reflex
bit-reproducible", but it means `Scared` is untested.

**Proposed:** Add a harness test that manually inserts `WatchedByPlayer(true)`, spawns a Smiley + a unit,
deals damage to the Smiley, and asserts the mood flips to `Scared` and the boss moves away from the unit
(flee vector is non-zero and points away). Use `SimConfig::deterministic_core()` so the trace is exact-
hashable; the test asserts the *direction* of movement, not the golden.

**Acceptance:** A regression that breaks `Scared` (e.g. wrong flee vector, mood never flips) reds this
test. The deterministic-core golden is unchanged (this test inserts a resource the golden run doesn't).

---

## Raise `GOLDEN_STABILITY_REPS` and/or test `train verify`'s cross-process flake detection

**Origin:** 2026-07-20 code review, L8 (`src/bin/train.rs:2210`, `GOLDEN_STABILITY_REPS = 3`).

**Why deferred:** The guard works today, but it's a thin sample for a guard that protects golden integrity.
The G0 bug was ~30% split under load; 3 intra-process reps would miss it ~34% of the time even if it were
intra-process (it wasn't — G0 was cross-process).

**The issue:** `recompute_goldens_stable` (`src/bin/train.rs:2185-2204`) only checks intra-process
stability (3 repeated builds must agree). The comment admits the cross-process flake ("the known-rare,
load-correlated CROSS-process flake... is not fully observable intra-process") and defers to manual
`train verify`. There is **no test that `train verify` actually catches cross-process flakes** — it's
procedure, not mechanism.

**Proposed (pick one or both — ask before implementing):**
1. **Raise `GOLDEN_STABILITY_REPS`** from 3 to e.g. 10. Cheap (intra-process), narrows the miss rate.
2. **Add a test that `train verify --reps K` catches a synthetic cross-process flake** — inject a flaky
   RNG path behind a test flag and assert `verify` reds. Bigger change, but mechanizes the cross-process
   guard.

**Acceptance:** Either the reps are higher, or the cross-process guard is mechanized (not just procedural).

---

## Finish the half-done `mycelia/mod.rs` directory split

**Origin:** 2026-07-20 code review, L12 (`src/mycelia/mod.rs`, 1647 lines).

**Why deferred:** The `mycelia/` directory already has `{control,field,pipeline,fruit,species,perceptual,
habitat,measure,agents}.rs` submodules — this is finishing a half-done split. Low risk, established
pattern.

**The issue:** `mycelia/mod.rs` (1647 lines) carries config (~270 lines: `MyceliaConfig` + `DampWeight` +
the three `validate_*` fns) + validation (~270 lines) + GPU setup/runtime (~700 lines) + a `MoldHabitat`
builder. The config block is a clean contiguous range (lines ~139–852) with no cross-dependencies into the
runtime block.

**Proposed:** Extract `mycelia/config.rs` (`MyceliaConfig` + `DampWeight` + the three `validate_*` fns,
~700 lines) and `mycelia/habitat.rs` (`MoldHabitat` + `setup_habitat`, ~60 lines). `mod.rs` then holds
only the plugin + GPU pipeline wiring. Mirrors the existing `mycelia/{control,field,pipeline,…}`
decomposition.

**Acceptance:** Pure code move — determinism-neutral. `cargo test` + `cargo test --features test-harness`
unchanged; `cargo check --release` clean, zero warnings.

---

## Finish the half-done `almond_water/mod.rs` directory split

**Origin:** 2026-07-20 code review, L12 (`src/almond_water/mod.rs`, 1135 lines).

**Why deferred:** The `almond_water/visual.rs` submodule already exists; only `mod.rs` lags. Same pattern
as the `mycelia/` split above.

**The issue:** `almond_water/mod.rs` (1135 lines) carries config (~280 lines: `AlmondWaterConfig` +
`AlmondWaterDynamics` + `validate_config`) + field impl (~400 lines) + plugin. The `visual` submodule
already exists.

**Proposed:** Extract `almond_water/config.rs` (`AlmondWaterConfig` + `AlmondWaterDynamics` +
`validate_config`, lines ~38–315). `mod.rs` keeps the field + plugin. Mirrors the `mycelia/` pattern.

**Acceptance:** Pure code move — determinism-neutral. `cargo test` + `cargo test --features test-harness`
unchanged; `cargo check --release` clean, zero warnings.

---

## Extract `apply_archive` RON-splice machinery from `bin/train.rs`

**Origin:** 2026-07-20 code review, L13 (`src/bin/train.rs`, 2563 lines).

**Why deferred:** The single highest-value split because it removes the most LOC from a file that has *no*
compile gate keeping it cohesive (it's a `[[bin]]`). The RON-splice machinery is pure text manipulation
with no `squad_ai`/harness dependency.

**The issue:** `bin/train.rs` (2563 lines) carries the CLI + 7 search drivers + RON-splice `apply_archive`
machinery (lines ~1538–2130, ~600 lines) + `recompute_goldens*`/`verify` block (lines ~2167–end, ~400
lines). The RON-splice code is pure text manipulation; the golden-management code is glue.

**Proposed:**
- Extract `squad_ai/apply.rs` (gated by `test-harness`) — the `apply_archive` RON-splice machinery
  (`splice_block`, `scalar_eq`, `block_scan_ignores_parens_in_comments_and_strings`, etc.). Move the
  existing tests with it.
- Extract `train/goldens.rs` (or `squad_ai/goldens.rs`) — `recompute_goldens_stable`, `verify`, the
  `GOLDEN_STABILITY_REPS` const.
- `bin/train.rs` becomes a thin CLI dispatcher (~600 lines).

**Acceptance:** Pure code move — determinism-neutral. `cargo test` + `cargo test --features test-harness`
unchanged (including `splicing_the_shipped_config_with_its_own_values_is_a_byte_identical_no_op`); `cargo
check --release` clean, zero warnings.

---

## Split `squad_ai/coevolve.rs` (1353 lines) into genome/descriptor/population

**Origin:** 2026-07-20 code review, architecture finding (`src/squad_ai/coevolve.rs`).

**Why deferred:** Three natural seams are already marked by section banners in the file. Splitting would
let the search-only `mod.rs` gate just the loop.

**The issue:** `coevolve.rs` (1353 lines) carries three concerns already marked by section banners:
- `Genomes + feasibility` (~260 lines)
- `Behaviour descriptors` (~90 lines)
- `Populations` + the co-evolution loop (~1000 lines)

**Proposed:** Split to `coevolve/{genome,descriptor,population}.rs`. `coevolve/mod.rs` (gated by
`test-harness`) re-exports and holds just the loop entry point. Move the existing 10 tests with their
respective concerns.

**Acceptance:** Pure code move — determinism-neutral. `cargo test --features test-harness` unchanged;
`cargo check --release` clean, zero warnings.

---

## `git rm` the tracked `config.ron.bak.pre-vitality`; add `**/*.bak*` to `.gitignore`

**Origin:** 2026-07-20 code review, L14 (`assets/config/config.ron.bak.pre-vitality`).

**Why deferred:** Trivial hygiene, but the file is 79 KB of stale config with no current purpose, and there
is no `.gitignore` rule preventing the next one.

**The issue:** `git ls-files` confirms `assets/config/config.ron.bak.pre-vitality` is tracked (committed in
`19f6c8b` "WIP snapshot before pull"). It's a pre-vitality backup of `config.ron`, dated 2026-07-13. The
`.gitignore` has rules for `elites_*.ron`, `baseline_prior.ron`, and `bake_history/`, but **no rule for
`*.bak.*`**. This is the kind of pre-change snapshot the "one path, no fallback" CLAUDE.md rule explicitly
argues against keeping around.

**Proposed:**
1. `git rm assets/config/config.ron.bak.pre-vitality`
2. Add `**/*.bak*` (or `assets/config/*.bak*`) to `.gitignore`.

**Acceptance:** `git ls-files | grep .bak` returns nothing. A future `config.ron.bak.*` is ignored.

---

## Update `docs/level-quality-checklist.md` seed list (retired seeds)

**Origin:** 2026-07-20 code review, L15 (`docs/level-quality-checklist.md:76`).

**Why deferred:** Doc drift. The checklist ships a copy-pasteable command using seeds the code no longer
uses.

**The issue:** `docs/level-quality-checklist.md:76` tells the reader to run the level search with:
```
cargo run --release --features test-harness --bin train -- levels \
  --generations 800 --batch 48 --seeds 0x5C09191,0xA11CE,0xBEEF
```
`0xA11CE` and `0xBEEF` are explicitly retired in `SEARCH.md` and `src/mold.rs:80` (the mold tipped them
into squad wipes). The checklist is dated `2026-07-15` and predates that retirement. The current held-in
seeds are `[0x5C09191, 0x1CE5, 0xFEED]` (per `src/mold.rs:81-83` / `SEARCH.md`).

**Proposed:** Update the seed list to `[0x5C09191,0x1CE5,0xFEED]`, or remove the example `--seeds` flag
and let the default apply.

**Acceptance:** The checklist's example command runs without using retired seeds.

---

## Update `README.md` ISSUES section (fixed items to DONE)

**Origin:** 2026-07-20 code review, M18 (`README.md:1-6` vs `README.md:13-22`).

**Why deferred:** Doc drift. At least 3 of 5 ISSUES are fixed but still listed as open.

**The issue:** README.md:1-6 lists 5 open ISSUES. Cross-reference with DONE (README.md:13-22) and code:
- **Issue #2 (wall corners)** — contradicted by DONE line 18: *"Wall corners no longer leave a post-shaped
  gap — a corner post fills the `WALL_THICKNESS²` column... posts squash with their neighbour knee walls
  under rotation (`src/dungeon.rs`)."*
- **Issue #4 (lamps floating)** — contradicted by DONE line 15 (TV/lamps/globe now
  `Role::Scatter(surface: "support")`) and `config.ron:92-100`'s comment (the "Ceiling Light.glb" was
  reclassified from `Anchor(Ceiling)` to `Scatter(surface: "worktop")`).
- **Issue #5 (trashcans in couches)** — contradicted by DONE line 20 (*"furniture–furniture overlap
  penalized"*) and `config.ron:212`'s `w_overlap: 10.0`. Whether the bin→couch overlap specifically is
  fixed needs visual confirmation, but the overlap-prevention machinery is in place.
- **Issues #1 (Smiley clipping walls) and #3 (squad stuck in doorways)** — no corresponding DONE entry;
  appear to remain open. (Issue #3 is the doorway look-ahead wedge — see the flowfield finding; it's a
  known limitation, not a regression.)

**Proposed:** Move #2, #4, #5 to DONE with their fix references. Keep #1, #3 if still real — verify with
a `devshot` playtest screenshot. For #3, reference the `LOOKAHEAD` limitation in `src/flowfield.rs:185-188`.

**Acceptance:** README ISSUES reflects current reality. A new contributor doesn't believe fixed bugs are
live.

---

## Create `AGENTS.md` or add a `## Lint` section to project `CLAUDE.md`

**Origin:** 2026-07-20 code review, config/docs finding.

**Why deferred:** The global `~/.claude/CLAUDE.md` says "add it to `AGENTS.md` so you will know to run it
next time" for lint/typecheck commands. Neither `AGENTS.md` nor a lint section in the project `CLAUDE.md`
exists, so the rule is silently unenforced.

**The issue:** The project `CLAUDE.md` documents testing (`cargo test`, `cargo test --features
test-harness`), determinism rules, screenshots, and game assets, but no `cargo clippy`/`cargo fmt`/
`cargo check` guidance. A contributor (human or agent) following the global rule has nowhere to record the
project's lint invocation.

**Proposed (pick one — ask before implementing):**
1. **Create `AGENTS.md`** at the repo root with the lint/typecheck one-liners: `cargo fmt --check`,
   `cargo clippy --all-targets --all-features`, `cargo check`, `cargo check --release`.
2. **Add a `## Lint` section to the project `CLAUDE.md`** with the same one-liners. Keeps everything in one
   file.

**Acceptance:** A contributor can find the lint commands without guessing.

---

## Remove orphaned asset directories

**Origin:** 2026-07-20 code review, render/assets finding.

**Why deferred:** Bloat, not correctness. The repo carries source art and abandoned kits that are never
referenced from code or config.

**The orphaned assets:**
- `assets/hazmat/`, `assets/hazmat_locomotion_pack/` — no reference from any `.rs`, `config.ron`, or test.
  `.blend`/`.fbx`/`.psd` source files sitting in the shipped assets tree. Appears to be abandoned
  character art.
- `assets/kenney_blaster-kit_2.1/` — no reference anywhere. Only `kenney_prototype-kit` is used (via
  `furniture_kenney.ron`).
- `assets/dimensional_crab/monster.{blend,fbx,obj,mtl}` + `Texture/` — the `.glb` is used
  (`crab/mod.rs:95`); the raw DCC source files are checked in alongside it. Source art usually lives
  outside `assets/`.
- `assets/almond_water_backrooms/scene.{gltf,bin}` — only the textures subfolder appears used; the gltf
  scene itself has no Rust loader.
- `assets/config/bake_history/` — contains one timestamped RON; not referenced from code, appears to be
  elite-search output that leaked into version control (though `.gitignore:47` ignores
  `assets/config/bake_history/` — verify it's not tracked).

**Proposed:** `git rm -r` the orphaned directories. Move DCC source art (`.blend`/`.fbx`/`.psd`/`.obj`/
`.mtl`) out of `assets/` to a separate `art_source/` directory (or just remove from the repo — source art
usually lives outside version control or in a separate art repo). Verify `assets/config/bake_history/` is
gitignored, not tracked.

**Acceptance:** `assets/` contains only runtime-loaded assets. `cargo check --release` clean (no missing
asset references).

---

## Deduplicate `almond_water.wgsl` noise functions into the shared library

**Origin:** 2026-07-20 code review, M4 (`assets/shaders/almond_water.wgsl:38-66`).

**Why deferred:** The deferred-from-2026-07-05-review item, still half-done. The 3D `hash13` is a
legitimate reason to diverge (the library only has `hash21`), but the 2D `vnoise`/`fbm` could import.

**The issue:** `almond_water.wgsl` re-declares `hash13` (a 3D variant), `vnoise`, and a 3-octave `fbm`
inline — defeating the shared `foundation::noise` library that `shader_lib.rs` says it eliminated "across
~7 files."

**Proposed:**
1. Extend `assets/shaders/noise.wgsl` with a `hash13` (3D hash) function.
2. Update `almond_water.wgsl` to `#import foundation::noise::{hash13, vnoise, fbm}` and remove the inline
   declarations. Keep only any truly divergent logic.

**Acceptance:** `almond_water.wgsl` no longer duplicates `vnoise`/`fbm`. The shader compiles (verify by
launching the windowed game and observing the Almond Water puddle). `cargo test` unchanged.

---

## `src/wfc.rs:259-260` — promote `assert!` to `Result` for caller-contract violations

**Origin:** 2026-07-20 code review, L2 (`src/wfc.rs:259-260`).

**Why deferred:** Latent — the placement solver's `n = tiled.len() + 1` (`placement/solvers/wfc.rs:76`)
could in principle exceed 31 with a future oversized furniture kit, but none exists today.

**The issue:** `assert!(n <= 32, "prototype set must fit in a u32 mask")` and
`assert_eq!(initial.len(), width * height, "initial domain size mismatch")` panic on caller contract
violations. Since `collapse_grid` already returns `Option<Vec<usize>>` for contradictions, these could be
`Result<_, String>` so a bad caller gets an `Err` instead of a panic.

**Proposed fix:** Change `collapse_grid` to return `Result<Vec<usize>, String>` and convert the two
`assert!`s to `Err(format!(...))`. Update callers (`wfc::generate` already handles `None`; extend to
handle `Err`).

**Acceptance:** A caller passing `n > 32` or a mismatched `initial.len()` gets a clean `Err`, not a panic.
`cargo test` unchanged (existing tests pass valid inputs).

---

## `src/crab/movement.rs:735-744` — pounce bite-victim selection uses exact-float equality

**Origin:** 2026-07-20 code review, M (`src/crab/movement.rs:735-744`).

**Why deferred:** Not a current bug — `nearest_planar` returns the exact `ptf.translation` it was fed, so
the `== tpos` equality happens to work. But it's a latent bug if any future code rounds `tpos`.

**The issue:** Pounce bite-victim selection compares `ptf.translation == tpos` after `nearest_planar`
returned a rounded key. If a future change rounds `tpos` (e.g. quantises to a cell), the equality breaks
and the bite silently no-ops.

**Proposed fix:** Carry the `Entity` through `nearest_planar`'s payload (as the boss/laser paths do) and
bite by entity, not by position equality. Small refactor of `nearest_planar`'s return type.

**Acceptance:** Pounce bite selection is by `Entity`, not float equality. Deterministic-core golden
unchanged (the shipped path produces the same entity today).

---

## `slop/` directory: rename or add a README explaining it's not a junk drawer

**Origin:** 2026-07-20 code review, architecture finding (`slop/`).

**Why deferred:** Naming, not correctness. But "slop" as a directory name is a project-name pun (the game
is "Foundation vs. Slop"), and a new contributor will reasonably assume it's disposable.

**The issue:** `slop/` contains useful session context — `dev_journal/` (9 dated session handoff notes),
`research/` (5 vetting/implementation notes), `ideas/` (1 file). The content is organized and tracked, not
cruft. But the name reads as "junk drawer."

**Proposed (pick one — ask before implementing):**
1. **Rename `slop/` to `notes/` or `devlog/`** — clearer to a new contributor. Requires updating any
   cross-references (grep for `slop/` in docs).
2. **Add a `slop/README.md`** stating it's the working-notes directory (dev journal + research + ideas),
   not waste. Keeps the pun.

**Acceptance:** Either the name is self-explanatory, or the README explains it. No code change.
