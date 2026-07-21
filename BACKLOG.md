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

## Fix almond-water resurrecting just-killed units

**Origin:** 2026-07-20 code review, H1 (`src/almond_water/mod.rs:866-889`).

**Why deferred:** The single clearest actionable bug in the review, but it touches the damage/heal
interaction and should ship with a focused test rather than a drive-by fix.

**The bug:** Damage sites (`src/laser.rs:457`, `src/crab/combat.rs:110`, `src/parasite.rs:1616`, and the
`smiley_zap`/`smiley_defense` assignment sites at `src/enemy.rs:1090,1153`) never clamp `health.current` at
0 — they leave it negative. `almond_water_effect` runs `.after(HealthDamage)`
(`src/almond_water/mod.rs:920-925`), so it sees the negative value. The heal computes
`want = (cfg.heal_rate * dt).min(health.max - health.current)` — with `current = -2`, `want = max + 2`,
a huge heal that clamps back to `max` after drinking. The unit returns to full HP; the despawn system reads
`hp.current > 0` next tick and **the unit survives**. A unit killed in a heal pool is resurrected.

**Proposed fix (pick one — ask before implementing):**
1. **Clamp damage at 0 in every damage site** — `hp.current = (hp.current - dmg).max(0.0)`. Broader change,
   touches 6+ sites, but kills the negative-HP class of bugs at the root.
2. **Gate the heal on `health.current > 0`** — add `if health.current <= 0.0 { continue; }` at the top of the
   per-candidate loop in `almond_water_effect`. Narrower, but leaves negative `current` observable to other
   readers (e.g. `forage_wounded_frac` at `src/crab/movement.rs:369` reads `fraction()` which clamps, so
   that's fine — but a future reader could regress).

Also worth a `max(0.0)` inside `Health::set`/the damage helpers if there is one, so the contract is
enforced at the type rather than at every call site.

**Acceptance:** A new liveness test — "a unit killed in a heal pool stays dead" — passes; the deterministic-
core golden either is unchanged (option 2) or moves *deliberately* and is re-pinned with a human-reviewed
diff (option 1, if the clamp changes a near-death tick boundary). `cargo test --features test-harness` clean.

---

## Fix `ImpactMaterial` / `BloodSprayMaterial` asset leaks

**Origin:** 2026-07-20 code review, M2 + M3 (`src/impact_fx.rs:161`, `src/gore.rs:792-818,1247-1356`).

**Why deferred:** Bounded in practice for pools (`max_pools: 300`) but unbounded for sprays over a long
fight. Not a correctness bug; a resource leak that degrades a long session.

**The leak:** `drain_impacts` calls `materials.add(ImpactMaterial{...})` per queued impact, spawning a new
`MeshMaterial3d` each time. `despawn_impacts` (`src/impact_fx.rs:177`) despawns the **entity** but never
removes the `ImpactMaterial` asset from `Assets<ImpactMaterial>`. Same shape in `gore.rs` for
`BloodSprayMaterial` (per-spray-event, entity despawns after `spray_duration` ~0.28s with no asset cleanup)
and `BloodPoolMaterial` (bounded at 300 but still no asset despawn). The codebase already fixed this exact
pattern once in `psi_vision` — `src/psi_vision.rs:30-33` explicitly calls out "the orphaned-material-per-
frame leak this codebase already had once." Propagate that lesson.

**Proposed fix (pick one — ask before implementing):**
1. **Despawn the asset alongside the entity** — in `despawn_impacts`/`despawn_gore`, also call
   `materials.remove(&material_handle)` before `commands.entity(e).despawn()`. Simplest; one extra call per
   despawn.
2. **Pool a small fixed set of materials** with mutated uniforms, like `psi_vision` does. More code, but
   zero allocation in the hot path.

**Acceptance:** `Assets::<ImpactMaterial>::len()` and `Assets::<BloodSprayMaterial>::len()` do not grow
unboundedly over a long liveness run (add a liveness assertion, or verify via `bevy/debug` resource
inspection). Deterministic-core golden unchanged.

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

## Fix `manca_embed` embedding into dying hosts

**Origin:** 2026-07-20 code review, H6 (`src/parasite.rs:1486-1503`).

**Why deferred:** Small edge case; a manca that wastes itself on a dying host is a gameplay nuance, not a
crash. But it's a one-line fix and the parasite lifecycle is a major system.

**The bug:** The `fresh` filter (`src/parasite.rs:1478-1482`) only checks `!inf.active`, not `hp.current > 0`.
A host at 0 HP (about to be despawned by the unit/crab despawn systems next tick) can still be embedded:
`hp.current -= embed_damage` drives it further negative, `inf.active = true`, and the manca despawns. The
host then despawns via its own death path, taking the `Infestation` with it — so the parasite is **lost**
(no gestation, no burst). A manca that embeds into a dying host wastes itself.

**Proposed fix:** Add `hp.current > 0.0` to the `fresh` snapshot filter, alongside the existing `!inf.active`
check. One line.

**Acceptance:** A liveness test where a manca is spawned adjacent to a 1-HP unit and a 0-HP unit: the manca
embeds into the live one, not the dead one. Deterministic-core golden may move *deliberately* (the manca no
 longer self-terminates on a dying host) — re-pin with a human-reviewed diff if so.

---

## Fix `crab_alarm_on_damage` false-alarming on heal ticks

**Origin:** 2026-07-20 code review, M8 (`src/crab/combat.rs:26-36`).

**Why deferred:** Narrow bug; a wounded crab standing in heal water is an edge case (crabs forage toward
water only when wounded, then drink it down). But the false-alarm bloom rouses nearby crabs on nothing,
which is a stigmergy-correctness issue.

**The bug:** `crab_alarm_on_damage` uses `Health::is_changed() && !hp.is_added() && hp.current < hp.max` to
detect damage. But `is_changed()` fires on *any* `Health.current` mutation — including the Almond Water
heal (`src/almond_water/mod.rs:875`), which writes `current` upward. A wounded crab standing in heal water
fires `is_changed()` **every heal tick**, flooding `ALARM` even though no damage happened. Same root cause
in `manca_rouse` (`src/parasite.rs:1162`).

**Proposed fix:** Gate on `current < last_seen` (the boss's `last_hp` idiom at `src/enemy.rs:1051`) — store
a per-crab `last_hp` and only fire when `current < last_hp`. Or compare against a stored previous value in
a side resource. Apply the same fix to `manca_rouse`.

**Acceptance:** A liveness test where a wounded crab stands in a heal pool for 60 ticks: `ALARM` deposits
are zero (no gunfire, no damage). Deterministic-core golden may move *deliberately* — re-pin with a
human-reviewed diff.

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

## Pin cross-plugin `HealthDamage` writers with explicit ordering

**Origin:** 2026-07-20 code review, M1 (`src/health.rs:62-71` + 6 damage sites).

**Why deferred:** Today reproducible (plugin order is stable across `App` instances built from the same
plugin list), so the deterministic-core exact-hash gate passes. But it's the same shape of trap the rest of
the codebase goes to great lengths to avoid, and a future refactor (adding a system to `HealthDamage`,
reordering a plugin tuple, moving a system into a different plugin) silently changes the composition order
with no compile-time or test-time guard.

**The issue:** The `HealthDamage` set (`src/health.rs:62-71`) carries "no ordering of its own, only a name
to sequence the heal behind." But its members all write `&mut Health` to overlapping entity sets with no
`.before()/.after()` between them:
- `crab_jump` — `src/crab/movement.rs:643`, registered at `src/crab/mod.rs:293`
- `crab_contact_damage` — `src/crab/combat.rs:71`, registered at `src/crab/mod.rs:298`
- `fire_laser` — `src/laser.rs:383-385`, registered at `src/laser.rs:163` (a *separate* plugin, registered
  after the `(ai, enemy, crab, nest, parasite)` tuple in `src/lib.rs:208-217`)
- `manca_embed` — `src/parasite.rs:1455`, registered at `src/parasite.rs:421`
- `parasite_burst` — `src/parasite.rs:1564`, registered at `src/parasite.rs:429`
- `smiley_zap` (assigns `= 0.0`) — `src/enemy.rs:1090-1091`, registered at `src/enemy.rs:393`
- `smiley_defense` (assigns `= 0.0`) — `src/enemy.rs:1153`, registered at `src/enemy.rs:397`

The concrete risk: `fire_laser` + `manca_embed`/`parasite_burst` all *subtract* from the same `Unit`'s HP.
Three-way float subtraction `((hp − a) − b) − c` vs `((hp − b) − c) − a` differs in the last bit
(IEEE-754 subtraction of distinct magnitudes is non-associative) — exactly the "non-associative `f32 +=`"
failure mode `field::sort_deposits` exists to prevent (`src/ai/field.rs:122-126`). On a near-death host
where the clamp at `≥ 0` engages, the last-bit difference flips whether the clamp fires, which can flip
whether the host dies this tick, which cascades into the snapshot hash.

**Proposed (pick one — ask before implementing):**
1. **`.chain()` the `HealthDamage` systems explicitly** — give each a written `.before()`/`.after()` so the
   order is a contract, not an accident. Verbose but localized.
2. **Give each damage system a disjoint entity filter** (e.g. `Without<CrabJumpedThisTick>`) so the borrow
   analysis can prove disjointness. More structural.
3. **Route all damage through a single accumulator system** that applies queued deltas in a sorted order —
   the same pattern used for `StigDeposits` / `GoreQueue`. Biggest change, but the most robust.

**Acceptance:** A new test that adds a 7th `HealthDamage` writer in a different plugin and reorders the
plugin tuple in `lib::run` — the deterministic-core golden must not move. (Today it would.)

---

## Make `cmaes.rs::repair` fail loudly on wrong-length input

**Origin:** 2026-07-20 code review, M13 (`src/squad_ai/cmaes.rs:141-145`).

**Why deferred:** Latent — current callers always pass full-length vectors. But a future caller passing a
short vector would silently corrupt the CMA update rather than failing loudly, violating the no-silent-
fallback rule.

**The issue:** `repair` clamps the index with `j.min(x.len().saturating_sub(1))`, repeating the **last
available element** for every missing dimension. The doc says "a shorter slice truncates to what the
emitter tracks," but the implementation fills with the last value, not truncates. A wrong-length input
manufactures false gradients (`x[last] - mean[j]` for the missing dims) instead of erroring.

**Proposed fix:** `if x.len() != self.n { return Err(format!("cmaes::repair: expected {} dims, got {}", self.n, x.len())) }`
at the top. One line.

**Acceptance:** A unit test passing a short vector gets a clean `Err`, not a corrupted update. Existing
tests unchanged (they pass full-length).

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

## Replace `DetRng::below(n=0)` silent-zero with `assert!(n > 0)`

**Origin:** 2026-07-20 code review, L1 (`src/rng.rs:35-43`).

**Why deferred:** Latent — every caller guarantees `n > 0` today. But the guard masks a caller bug rather
than surfacing it, which is the opposite of the project's fail-loudly rule.

**The issue:** `DetRng::below(n=0)` returns 0 silently. The comment says "guard so a bug can't panic" —
but the project's stated philosophy (`CLAUDE.md`) is "fail loudly." A caller bug (passing 0) looks like a
valid zero-draw instead of a crash.

**Proposed fix:** Replace `if n == 0 { return 0; }` with `assert!(n > 0, "DetRng::below(0): degenerate range at {}:{}", file!(), line!())`
— or `debug_assert!` to match the `sort_total!` discipline (panics under debug/test-harness, elided in
release). Prefer the plain `assert!` since this is a caller contract, not a hot path.

**Acceptance:** `cargo test` + `cargo test --features test-harness` unchanged (no caller passes 0 today).
A future caller bug crashes loudly.

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

## Smiley flee-vector edge case: `Vec2::ZERO` when no units alive

**Origin:** 2026-07-20 code review, M6 (`src/enemy.rs:1060-1064` + `enemy_seek:665-679`).

**Why deferred:** Edge case — all units dead means the player has lost, so the boss standing still during
"flee" is rarely observed. But it contradicts the "runs away" spec.

**The issue:** `flee_timer` is set to `scared_time` whenever `looked_at && attacked`. `enemy_seek`'s
`SmileyMood::Scared` arm re-resolves the flee vector from `nearest_unit` each tick. If `nearest` is `None`
(all units dead), `away = Vec2::ZERO` and the boss stands still during "flee" — contradicts the "runs away"
spec.

**Proposed fix:** Fallback to `motion.heading` when `nearest` is `None`, so a "scared" boss at least keeps
moving. One line.

**Acceptance:** A liveness test where all units are killed while the boss is `Scared`: the boss keeps
moving (non-zero velocity), not standing still. Deterministic-core golden may move *deliberately* — re-pin
with a human-reviewed diff.

---

## `crab_feeding_sates_hunger`: clamp `HUNGER` to `[0,1]`

**Origin:** 2026-07-20 code review, M9 (`src/crab/combat.rs:46-58`).

**Why deferred:** `Curve::eval` clamps the output so scoring is fine, but the unclamped drive value could
fold into `snapshot_hash` if `Drives` is hashed, causing two runs to diverge if one clamps and the other
doesn't.

**The issue:** `crab_feeding_sates_hunger` subtracts `hunger_sate_rate * dt` but `HUNGER` is never clamped
to `[0,1]` here. If `h` is already 0 (a fed crab that just started biting), `h - rate*dt` goes negative.
`Behavior::score` multiplies considerations including `Curve::Linear { m: 1.0, b: 0.2 }` on HUNGER
(`brain.rs:566`), and `Curve::eval` clamps the **output** to `[0,1]` (`utility.rs:212`), so a negative
HUNGER still scores 0.2 — fine for scoring. But the unclamped drive value itself is the risk.

**Proposed fix:** Add `.max(0.0)` in `drives.set(HUNGER, h - rate*dt)`, or a clamp inside
`crab_feeding_sates_hunger`. One line.

**Acceptance:** `HUNGER` never goes negative. Deterministic-core golden unchanged (the clamp is a no-op
for the shipped config, which doesn't produce negative HUNGER today).

---

## Nest `spawn_boost` decay should pause while crowded/capped

**Origin:** 2026-07-20 code review, M10 (`src/crab/foraging.rs:857-865`).

**Why deferred:** Minor — probably intended (a nest that can't birth shouldn't hoard boost forever). But
it defeats the "well-fed → fast" intent under sustained crowding.

**The issue:** `nest.spawn_boost = (nest.spawn_boost - spawn_boost_decay * dt).max(0.0)` runs
unconditionally, then `respawn_timer -= dt` and the `if respawn_timer > 0 { continue }` skips the rest.
The comment at `863-865` says "Re-arm even if this tick can't spawn (cap/crowd), so a fed nest keeps its
fast cadence" — and it does re-arm `respawn_timer` at the boosted rate. But the boost **itself** decays
during the wait, so a crowded nest's boost drains while it waits for crowd to clear.

**Proposed fix (ask before implementing — this may be intended):** Move the `spawn_boost` decay *after*
the cap/crowd/can-spawn guards, so it only decays when the nest actually births. Or gate the decay on
`density < crowd_cap && total < crab_count_max`. Or document that the decay-during-wait is intended.

**Acceptance:** Either the boost stops decaying while blocked, or the behavior is documented as intended.
Deterministic-core golden may move *deliberately* — re-pin with a human-reviewed diff.

---

## `despawn_dead_nests`: add `sort_total!` for determinism consistency

**Origin:** 2026-07-20 code review, M11 (`src/nest.rs:251-256`).

**Why deferred:** Two nests dying on one tick is rare (laser hits one at a time), and `despawn` order only
affects entity-id reuse. But every other despawn owner in the codebase sorts for determinism; this one is
the outlier.

**The issue:** `despawn_dead_nests` uses raw query order for despawn — no `sort_total!`, unlike
`crab_despawn_dead` (`src/crab/combat.rs:139`, sorts by `CrabSeed`), `despawn_dead_units` (`src/squad.rs:
574`, sorts by `SquadMember`), and `despawn_dead` (boss, `src/enemy.rs:910`, sorts by position bits). If
two nests are ever razed on the same tick (e.g. a future AoE), the entity-id reuse order would be query-
order-dependent.

**Proposed fix:** Add `sort_total!(&mut dead, |n| n.0.to_bits())` (sort by `Entity` bits) before the
despawn loop, matching the other despawn owners' discipline. One line.

**Acceptance:** `cargo test` + `cargo test --features test-harness` unchanged (no two nests die on one
tick in the shipped goldens). Consistency with the other despawn owners.

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

## Fix `Health` lower-clamp: `current` can go negative and stay negative until despawn

**Origin:** 2026-07-20 code review, M (health.rs) — related to H1 (almond-water resurrection) but broader.

**Why deferred:** The almond-water resurrection (H1) is the acute symptom; this is the root cause. Fixing
H1 alone (heal gate) leaves negative `current` observable to other readers. Fixing this alone (clamp
damage) fixes H1 at the root.

**The issue:** `Health` has no lower clamp; `current` can go negative and stay negative until despawn
(`src/health.rs:18-37`). Damage sites at `laser.rs:457`, `combat.rs:110`, `parasite.rs:1616` never clamp
at 0. The despawn filters check `hp.current <= 0.0`, so a negative value still triggers death. But between
the killing tick and the despawn tick, a negative `current` is **observable** to systems that read
`Health` directly — most acutely the almond-water heal (H1), but also `forage_wounded_frac`
(`src/crab/movement.rs:369`, which reads `fraction()` that clamps, so that's fine) and any future reader.

**Proposed fix:** Clamp `hp.current = hp.current.max(0.0)` in every damage site, OR add a lower clamp
inside `Health`'s mutation API (if there is one — check `health.rs` for a `damage`/`set` helper). This is
the root-cause fix for H1; the heal gate is the defense-in-depth fix.

**Acceptance:** `Health.current` is never negative. The almond-water resurrection (H1) is impossible even
without the heal gate. Deterministic-core golden may move *deliberately* (clamping changes a near-death
tick boundary) — re-pin with a human-reviewed diff.

---

## `src/bin/train.rs:833,836` — `unwrap_or(0)` on PROGRESS/RESULT token parse

**Origin:** 2026-07-20 code review, L5 (`src/bin/train.rs:833,836`).

**Why deferred:** Not a correctness issue — best-effort progress-bar driving only, and the fitness `best`
is `Option<f32>` with the winner selection handling `None`. But a `warn!` would help diagnose a
misbehaving island.

**The issue:** `g = cur.parse().unwrap_or(0); b = v.parse().unwrap_or(0.0)` silently falls back to 0 on
malformed PROGRESS/RESULT tokens. A misbehaving island that emits garbage progress lines looks identical to
a healthy one making no progress.

**Proposed fix:** Replace `unwrap_or(0)` with `unwrap_or_else(|e| { warn!("island {i}: malformed PROGRESS token {tok:?}: {e}"); 0 })`.
One line each.

**Acceptance:** A misbehaving island's garbage tokens are logged. No behavior change for healthy islands.

---

## `src/squad_ai/parallel.rs:273-274` — cap `vec![0u8; n]` allocation from wire-supplied length

**Origin:** 2026-07-20 code review, L4 (`src/squad_ai/parallel.rs:273-274`).

**Why deferred:** Threat model is "worker bug → driver OOM" (workers are local child processes the driver
spawned), not external attack. But a corrupted worker frame could request up to 4 GiB allocation before
the body read fails.

**The issue:** `read_frame` reads a `u32` length from the wire, then `vec![0u8; n]` where `n =
u32::from_le_bytes(len) as usize`. No upper bound on `n` — a corrupted/malicious worker frame could request
up to 4 GiB allocation.

**Proposed fix:** Add a sanity cap (e.g. 64 MiB): `if n > 64 * 1024 * 1024 { return Err(format!("worker frame length {n} exceeds 64 MiB cap")) }`.

**Acceptance:** A corrupted worker frame with a huge length field fails loudly with a clean `Err`, not an
OOM. No behavior change for well-formed frames.

---

## `src/light.rs:521-555` — eliminate per-tick `Vec<FlashlightCone>` allocation

**Origin:** 2026-07-20 code review, L6 (`src/light.rs:521-555`).

**Why deferred:** Performance polish — usually 0-1 cones so the Vec is tiny, but it's a per-tick heap
alloc on the pinned path (`FixedUpdate` runs 60 Hz).

**The issue:** `apply_dynamic_lights` allocates `let mut cones: Vec<FlashlightCone> = ...collect()` every
fixed tick. Usually 0-1 researchers so the Vec is tiny, but it's a per-tick heap alloc on the deterministic
path.

**Proposed fix:** Use a `SmallVec<[FlashlightCone; 2]>` (if `smallvec` is already a transitive dep — check
`Cargo.lock`), or a `Local<Vec<FlashlightCone>>` reused buffer cleared each tick. One-line change.

**Acceptance:** `apply_dynamic_lights` no longer allocates per tick. Deterministic-core golden unchanged.

---

## `src/light.rs:906-910` — `attach_fixture_lights` entity-id seed is run-dependent

**Origin:** 2026-07-20 code review, L7 (`src/light.rs:906-910`).

**Why deferred:** Cosmetic-only (never touches `LightField`/`snapshot_hash`), so it doesn't break
determinism. But if a test ever screenshots the lighting it could be flaky.

**The issue:** `attach_fixture_lights` hashes `e.to_bits() as u32` for the flicker seed. Entity ids are
not guaranteed stable across runs (they depend on spawn order/allocator), so the `failing`-tube selection
and `phase` are technically run-dependent.

**Proposed fix (ask before implementing):** Either (a) derive the flicker seed from a stable property
(e.g. the fixture's cell coordinate or a spawn-order index), or (b) document that flicker is intentionally
run-dependent (cosmetic-only) and accept the screenshot-test flake risk.

**Acceptance:** Either flicker is reproducible across runs, or the run-dependence is documented and a
screenshot test uses a tolerance.

---

## `src/squad.rs:441-531` — warn when squad spawns <5 members

**Origin:** 2026-07-20 code review, L3 (`src/squad.rs:441-531`).

**Why deferred:** Silent degradation, not a panic. But a degenerate dungeon that yields <5 floor cells in
the spawn spiral silently spawns fewer than 5 squad members with no `warn!`.

**The issue:** `OUTFITS[i]`, `RoleId::ALL[i]`, `personas[i]` where `i` comes from `cells.iter().enumerate()`
and `cells: Vec<IVec2>` is `take(5)`. Arrays are all `[T; 5]`. Safe because `i < cells.len() ≤ 5`. But if
the dungeon's spawn neighborhood yields <5 floor cells, the squad silently spawns fewer than 5 members.

**Proposed fix:** Add `if cells.len() < 5 { warn!("squad spawn: only {} floor cells in spawn spiral, spawning {} members", cells.len(), cells.len()) }`
after the spiral search. One line.

**Acceptance:** A degenerate dungeon logs the warning. No behavior change for a normal dungeon.

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

## `src/parasite.rs` — reconcile docstring contradiction on whether the host dies

**Origin:** 2026-07-20 code review, parasite finding (`src/parasite.rs:5-8` vs `:156-163`).

**Why deferred:** Doc inconsistency, not a code bug. But the two docstrings disagree, and a reader can't
tell which is authoritative.

**The issue:** The module-level docstring (`parasite.rs:5-8`) says the parasite *"then bursts out —
killing the host"*. The `parasite_burst` docstring (`:156-163`) says *"the host survives, wounded"* — the
burst deals `hp.max / 3.0`, which is deliberately NOT an instakill (unless the host is below ⅓ HP, in which
case it *is* an instakill). The two docstrings disagree.

**Proposed fix:** Reconcile the docstrings — either the host survives (matching the `hp.max / 3.0` code)
or the host dies (matching the module docstring, which would require the code to deal `hp.current` damage
or clamp-kill). Ask the user which is authoritative before changing code.

**Acceptance:** The module docstring and the `parasite_burst` docstring agree. If the code changes (host
dies), the deterministic-core golden moves *deliberately*.

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
