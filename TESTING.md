# Testing — strategy, how-to, and reference

The single source of truth for this repo's test system: *why* it's shaped the way it is, *how* to run and
extend it, and a *reference* for the harness API and invariants. Read the strategy section first — one wrong
oracle choice is the difference between a golden regression net and a test that flakes every fifth run.

---

## TL;DR

```bash
cargo test                                                # deterministic core — fast, GPU-free, the CI hard gate
cargo test --features test-harness -- --test-threads=1    # + headless replay / liveness / SSIM (GPU-free)
```

- **`cargo test`** runs the pure-logic + golden layer (RNG, WFC, utility-AI, ORCA, laser, geometry,
  placement). No GPU, no window, ~instant. **This is what CI blocks on.**
- **`--features test-harness`** additionally boots the *real game* headless and runs replay + liveness +
  visual tests. They open no window and need **no GPU**: the harness sets `RenderPlugin` with
  `backends: None`, so every render type is registered but no adapter/device is ever created.

---

## Strategy: the two-altitude model (read first)

The single most important fact: **the gameplay logic is bit-reproducible; the Avian physics layer is not.**
So there are two altitudes of oracle, and using the wrong one gives you a flaky test:

| Layer | Reproducible? | Oracle | Example test |
|---|---|---|---|
| Gameplay **logic** — AI, movement, combat, economy, WFC, fields, placement | **Yes**, bit-for-bit (fixed dt + one thread + seeded RNG) | **exact hash / golden value** | `deterministic_core_is_bit_identical` |
| **Physics** — gib chunks (Avian solver) | **No** — floats aren't bit-stable even single-threaded (documented invariant) | **liveness / tolerance** | `full_sim_stays_live` |
| **Render/FX** — gore, juice, VHS, blood-lens | No (driven by physics floats) | **perceptual (SSIM)** | `visual_regression::ssim` |

If you try to exact-hash a physics-on run, it passes ~20% of the time and wastes your afternoon. Hash the
**`deterministic_core()`** config (physics off); use liveness for everything else.

**Oracle rules — pick by determinism class:**

- **Deterministic core** (WFC output, full-sim gameplay state, utility-AI scores, ORCA/flow-field vectors,
  placement layouts): **exact hash / golden value.**
- **Physics gore** (gib / juice / VHS / blood-lens): **perceptual / tolerance** (SSIM with a threshold).
  Never exact.
- **Agent exploration**: **liveness / distribution** — no panic, no NaN transform, stuck-rate under
  threshold, coverage %.
- Golden values are committed. Changing one is a **deliberate, human-reviewed** act — never auto-approve a
  diff. Prefer a human-readable golden (a hash *and* the source values) so the diff is reviewable.
  **`train apply` enforces this**: it recomputes the goldens, and if they MOVED it aborts and reports
  `old -> new` rather than re-pinning. `--repin-goldens` accepts the move; the unattended callers inside
  `cargo train all` never pass it, so a bake that changes the shipped sim stops for a human. This is not
  belt-and-braces — `apply` used to re-pin silently, and on 2026-07-16 that turned five correctly-failing
  tests green against a machine-baked level (see the incident log at the top of `tests/replay.rs`). **A tool
  that both changes the sim and moves the ruler in one step cannot be reviewed.**

---

## Invariants & determinism rules (never violate)

These are the hard-won constraints. Violating any one either flakes the suite or silently corrupts a golden.

1. **Physics off for exact hashing.** Use `SimConfig::deterministic_core()` for any `snapshot_hash`
   equality assertion. `SimConfig::default()` (physics on) is liveness-only.
2. **Gameplay is solver-free.** Units, enemies, and lasers use custom movement and never touch the Avian
   solver; **only gib chunks are `RigidBody::Dynamic`.** Do not add gameplay entities to the physics world
   to make them testable, and **never hash gib/physics transforms** — they aren't bit-reproducible.
   `snapshot_hash` queries `(&Transform, &Health)`; gibs have no `Health`, so they're excluded
   automatically. Don't add them.
3. **No entropy — seed-driven RNG only.** The gameplay layer uses `util::rand01` (LCG) and
   `util::hash01_u32` (Wang mix); the generation/solver stack uses `rng::seeded(...)` → `DetRng` (ChaCha8,
   via `rand_chacha`), pinned in `tests/rng_guard.rs`. **Never** introduce `thread_rng`, `getrandom`,
   `OsRng`, or `from_entropy`. The **one sanctioned use of the `rand` ecosystem** is the placement-grammar
   solver (`src/placement/`), which draws from a **seeded** `rand_chacha::ChaCha8Rng` — no entropy, fully
   reproducible. New per-agent randomness threads a `u32` seed through `util::rand01`, or a seeded
   resource (see `LaserRng`).
4. **One App at a time.** Two headless Apps in one process share Bevy's global task pool (and, when a
   backend exists, the GPU device) and interfere. Every harness test takes `let _serial = serial_guard();` first and holds it for the App's
   lifetime. (`--test-threads=1` in CI is belt-and-suspenders; `serial_guard` alone is sufficient.)
5. **Single-threaded.** `build_headless_app` forces the compute pool *and* rayon to one thread before any
   plugin initializes. Multithreading, rayon work-stealing, and concurrent Apps each break determinism —
   that's why all three are pinned.
6. **Fixed dt.** The pinned simulation runs on **`FixedUpdate`** at 60 Hz (`lib.rs`:
   `Time::<Fixed>::from_hz(60.0)`; `AiSet` and field diffusion are registered on `FixedUpdate` in
   `ai/mod.rs`). The harness drives real time by exactly `fixed_dt` per `step` (`TimeUpdateStrategy`), matched
   to the `Time<Fixed>` timestep, so the sim never sees variable pacing — even though `field.rs` still
   integrates by `time.delta_secs()`, that delta is now fixed.
7. **Test only compiled code.** The crate is a **lib + bin split** — domain modules are declared in
   `src/lib.rs`. `src/combat.rs` and `src/enemies.rs` are shelved (not declared) — do not write tests
   against them. The live enemy path is `enemy.rs` + `crab.rs`.
8. **A determinism probe on an IDLE box proves nothing.** Order-dependence bugs are races: with G0 *live*,
   an idle machine produced 12/12 identical rollouts in one process and 5/5 across fresh processes, and
   only split under CPU load. Any test asserting same-seed reproducibility of a *long, combat-carrying*
   run must generate background load — see `search_rollouts_are_reproducible_under_load`. Without it the
   test is decoration. (The short physics-off goldens don't need this: they're a fixed 180/1800-tick
   trajectory that never enters the racy paths.)
9. **Exercise the code you mean to pin.** G0 lived in `laser::fire_laser` and survived for months *behind*
   a 24-build determinism gate, because that gate runs 180 ticks with no synthetic player — the squad idles
   at spawn and never fires. Coverage of a *system* is not coverage of its *contended* path. When a guard
   is meant to pin ordering, check that the scenario actually produces >1 concurrent actor in that code.
10. **Every sort declares its determinism contract — enforced, not commented.** ECS query order is not
    stable across `App` instances, so a sort whose key ties falls through to it and the sim stops being
    reproducible. Pick one, explicitly: **`sort_total!(&mut v, |x| key)`** when order is load-bearing (a
    greedy loop, a `take(n)` budget, a shared RNG draw or counter, a clamped accumulate, a lethal pick) — it
    panics under `test-harness`/debug naming the site and the duplicated key; **`util::sort_value_canonical`**
    when tied elements are genuinely *interchangeable* — sort by the WHOLE value, never a prefix, so a tie
    means they are identical; or **`// SORT-OK: <reason>`** when the input never comes from an ECS query.
    `tests/determinism_lint.rs` blocks an unannotated sort in the hard gate.
    This exists because prose failed: three sites (`almond_water_effect`, `enemy::smiley_defense`, the ORCA
    neighbour sort) *asserted* a total order in a comment while keying on a prefix of the value, and each
    fell into the exact trap it described. **Sorting by a prefix is the single most common shape of this
    bug** — `(pos)` when the element is `(pos, payload)`, so coincident actors tie and the payload decides
    something. Crabs `clamp_to_patch`-ed against a wall hold *bit-identical* coordinates, so this is routine,
    not theoretical (measured: 6 fully-tied pairs at one tick).
11. **An exoneration is only as strong as the condition it was measured under.** Corollary of 8, and the
    rule that keeps a ruled-out list honest. "I removed the suspect and it still diverged" does **not** clear
    the suspect — with several order-dependencies live, removing one leaves divergence (the aim-scatter A/B
    was read as REFUTED this way, and it was the actual root cause). "It didn't reproduce over N runs" does
    **not** clear a hypothesis unless the box was loaded. Any row in a ruled-out table must record *how* it
    was measured, or it is not evidence — two rows in the G0 doc had to be struck for exactly this.

---

## What's in the box

### 1. Deterministic-core layer (`cargo test`, GPU-free)

Pure functions called directly — **no Bevy `App`**. Fast, deterministic, no GPU. This is the CI hard gate.
See the **Test inventory** below for the full per-module breakdown.

### 2. Headless replay harness (`--features test-harness`, GPU-free)

`src/sim_harness.rs` boots the **real game plugins** with no window and no wgpu backend (`WinitPlugin`
disabled; `RenderPlugin { backends: None }`). `tests/replay.rs` and `tests/liveness.rs` run against it.

Dropping the backend is sound, not a shortcut: `snapshot_hash` covers `(Transform, Health)`, every writer
of which is on `FixedUpdate`, and rendering only *reads* sim state. It was verified by measurement — with a
real Metal backend and with no backend, seed `0x5C09191` × 1800 ticks hash to the same value, with the whole
suite passing. That deterministic-core golden is now pinned as an absolute value by
`migrated_defaults_reproduce_the_shipped_golden_hash` (currently `0x6716f1718a9774d1`; it tracks gameplay, so
committing a deliberate balance change updates it — earlier values like `0xec1add310772895c` /
`716d0cfbb69b778e` predate the faction-relative-fear / psionic-field-sight / SCP-150 work and are stale). The
field-grid oracle `field_passes_are_bit_identical` is pinned separately (`0x5d60_2962_2213_5600`). It also made the harness ~2.9× faster (that episode:
9.31 s → 3.18 s), because ~84% of a headless run was render-extract rather than simulation.

### 3. Visual regression (`src/visual_regression.rs`)

Hand-rolled **SSIM** (`ssim(a, b, w, h) -> f32`, `1.0` = identical) for the FX layer — compare a screenshot
to a committed golden with a tolerance (`>= 0.98`), never exact bytes. The math is dependency-free and
unit-tested; the *capture* half needs the windowed game + `devshot` (the harness has no window) — see
"Constraints & not-yet-automated".

---

## Test inventory

The canonical map of what pins what. Update this table when you add or retire a test module.

### In-file `#[cfg(test)] mod tests` — pure logic, `cargo test`

| Module | What it pins |
|---|---|
| `ai/utility.rs` | Response curves (linear/power/logistic/step), dual-utility bucket selection, weighted-random intra-bucket pick, `decide` determinism, safety default. |
| `orca.rs` | `new_velocity` ORCA properties — free agent keeps preferred velocity, output clamped to max speed, head-on pairs deflect, speed bound holds, deterministic. |
| `laser.rs` | CPU raycast geometry — sphere/capsule hit & miss, capsule taller than sphere, deterministic. |
| `wfc.rs` | Grid + graph WFC — always-compatible alphabet collapses, contradiction detection, seed determinism, golden draw-order grid, boundary links stay on-grid, degree-cap, isolated nodes, corridor-favouring weights. |
| `dungeon.rs` | Shipped RON parses & generates, per-config determinism, region type tags, room-fit/margin, error-not-panic on bad config, liminality behaviour, Grid vs Graph topology connectivity/non-overlap, and a **golden dungeon snapshot** (`golden_dungeon_snapshot_is_stable`). |
| `geom.rs` | Poisson-disk sampling (determinism, spacing, bounds), Delaunay triangulation (small cases, every-point-a-vertex, determinism), degree-prune. |
| `autogib.rs` | Fracture **topology** — slice/cap geometry, reaches target & deterministic, missing UV/normals synthesized, open-boundary dropped, degenerate plane leaves piece whole. (Counts/structure, never float vertex positions.) |
| `crab.rs` | Crab floor-patch clamp geometry — a crab is never inset into a wall slab (the reported wall-clip bug); walled edges inset, open edges keep full extent. |
| `almond_water/mod.rs` | The Almond Water field math — `drink` drains exactly & clamps at 0, `tick` accumulates toward the seep/evaporate fixed point & clamps to capacity, diffusion spreads to a neighbour & conserves between two cells, and `validate_config` accepts a valid slice & rejects out-of-range (diffuse/ior/capacity/wounded-frac/negative-seep). |
| `visual_regression.rs` | The SSIM oracle itself — identical→1.0, tiny perturbation stays > 0.98, structural change scores low, symmetric & deterministic. |
| `placement/manifest.rs` | RON manifest parsing — roles & affordances parse, too-many-tiled is rejected. |
| `placement/solver.rs` | Role→solver routing — routes by candidate role, registration-order-independent, unhandled role → no route, empty candidates → empty success, post-route constraint guard, mixed hard/soft requirements. |
| `placement/furnish.rs` | Room furnishing — typed rooms pick matching kits (a living room gets a seat + screen), same-type rooms can differ, untyped rooms top-up, freestanding constraints are kit-agnostic and spread. |
| `placement/solvers/wfc.rs` | Tiled-WFC placement — stays inside the region rect, no candidates → empty, deterministic under a seed. |
| `placement/solvers/metropolis.rs` | Metropolis layout — objects stay inside & non-overlapping, deterministic under a seed. |
| `placement/solvers/constraint.rs` | Door constraints — exactly one door per room, count places distinct doors, over-count clamped to sites, deterministic selection. |
| `squad_ai/level_genome.rs` | The level genome (dungeon/furniture/mushroom config) — `authored` decodes to the shipped config within f32 precision, mutation stays feasible across 300 draws (every subsystem validator passes), a mutation actually moves a gene, and dropping every room type still keeps ≥1 (+ a matching damp table). |
| `squad_ai/level_quality.rs` | The static level-quality objective — a disconnected level fails the minimal criterion (fitness `None`), one room is fully reachable, infestation & room/corridor split read correctly from the habitat mask, band/reward helpers behave. |
| `squad_ai/level_eval.rs` | The generate-and-measure evaluator (GPU-free) — the shipped level scores in (0,1] and evaluates **reproducibly**; mutated genomes either score or cleanly reject (never panic). Runs the real `Dungeon::generate` / `furnish_all` / `habitat::build` pipeline. |
| `squad_ai/level_search.rs` (`test-harness`) | The level MAP-Elites loop — a short search fills ≥1 archive niche and its archive doc serialises to readable RON. |
| `squad_ai/behavior_genome.rs` | The 89-knob behaviour genome (`behavior:` config subset) — `authored` round-trips exactly, sits inside `BOUNDS` & is feasible, mutation stays feasible across 500 draws, a mutation moves something, wrong-length rejected. |
| `squad_ai/policy.rs` | The policy seam — `Observation` tensor has a stable dim, `UtilityPolicy` matches engine `decide`, `ScriptedPolicy` clamps, and the learned `NeuralPolicy` has a weight count matching its layers + a **deterministic, in-range argmax** `choose` (RNG-independent — exact-hash safe). |
| `squad_ai/policy_genome.rs` | The neuroevolution weight-vector genome — `authored` is deterministic, feasible, and decodes to an MLP; mutation stays in `[-W,W]` & feasible across 200 draws; wrong-length / out-of-range rejected. |
| `squad_ai/interest.rs` | The human-interest proxies — a blowout has ~0 suspense, a back-and-forth fight out-suspenses it, a comeback registers outcome-surprise, an efficient recovery beats a flat walkover on effectance, all terms bounded `[0,1]`. |
| `squad_ai/cmaes.rs` (`test-harness`) | Separable CMA-ES — **converges on a sphere & contracts sigma** (the correctness check), deterministic from its seed, a short generation is ignored. |
| `squad_ai/map_elites.rs` (`test-harness`) | The CMA-ME improvement-emitter loop (`map_elites_cma_loop`) illuminates several archive cells on a synthetic QD problem. |
| `squad_ai/poet.rs` (`test-harness`) | POET — **open-endedly grows harder niches & more skilled agents** on a synthetic difficulty/skill problem, rejects a hopeless seed pairing loudly, and `learning_progress` tracks recent improvement. |
| `elite_overlay.rs` | Evolved-elite runtime overlay (`FVS_*_ELITE`) — `parse_cell`/`parse_spec`, the minimal `Archive` mirror **ignores unknown archive fields** (the load-bearing serde assumption), pick-by-cell vs best-fitness selection, absent-cell / empty-archive rejected loudly. |

> **Offline training / search (`train` binary, `test-harness`).** The `train` subcommands drive these:
> `bench`/`probe`/`prior` (measure + freeze the baseline), `evolve3` (squad×swarm×world co-evolution),
> `levels`/`audio`/`behavior` (single-population MAP-Elites), **`rl`** (neuroevolution over `NeuralPolicy`
> weights; `--cma` uses the CMA-ME emitter), and **`poet`** (open-ended world×squad co-generation). None ship
> in the game binary — the runtime only reads the committed `elites_*.ron`. `rl`/`behavior`/`audio`/`poet`
> need `train prior` first, and the prior must be regenerated (`train prior`) after any `Mode`/`MODE_COUNT`
> change. `NeuralPolicy::choose` is argmax-deterministic so it stays on the exact-hash path.

### `tests/` integration files

| File | Gate | What it does |
|---|---|---|
| `tests/determinism_lint.rs` | GPU-free (no feature) | **The class-level guard.** Scans `src/` and fails any raw `sort*` that hasn't declared a determinism contract (`sort_total!` / `sort_value_canonical` / `// SORT-OK:`). Instant. Catches the whole family of "sort key ties → falls back to ECS query order" bugs at review time, where seven hand-fixes only caught instances. Its runtime half is `util::sort_total_by_key_at`, which panics naming the site + duplicated key the moment a tie occurs under the harness — reintroduce the `smiley_defense` cull bug and it reds in ~2 s. |
| `tests/rng_guard.rs` | GPU-free (no feature) | Freezes the exact bit output of every generator — `util` (`next_u32`, `rand01`, `hash01_u32`), `autogib::hash_f32`, and `rng::seeded` ChaCha8 (`raw_u64`, `unit`, `below`). A silent constant change trips here first. |
| `tests/wfc_pin.rs` | GPU-free (no feature) | Golden FNV-1a hash of `wfc::generate` over a 5-seed corpus + in-process reproducibility + the "a floor link only ever joins two floors" invariant. |
| `tests/replay.rs` | `test-harness` (GPU-free) | Boots the sim; same-seed → identical `snapshot_hash` on the core (`deterministic_core_is_bit_identical`); state evolves; the speed knob is deterministic (does **not** assert cross-speed equality); full-sim liveness. Also **`search_rollouts_are_reproducible_under_load`** — the G0 guard: 12 `rollout()`s at the search's real 7200-tick episode **with the synthetic player**, on **both held-in seeds**, must agree bit-for-bit. ~6 min. It runs the AUTHORED genome, which is NOT what the search evaluates — see the mutant guard below. It had been green for months on ONE seed while the other split 3 ways, which is why it now runs both. |
| `tests/replay.rs` (mutant guard) | `test-harness` (GPU-free) | **`search_rollouts_of_mutants_are_reproducible_under_load`** — 8 mutants × 3 reps × **both** held-in seeds × 7200 ticks, squad+swarm+world mutated, under load. ~40 min. **This is the guard that matters**: the authored genome is the one configuration the search never evaluates, and a mutant reaches code the authored config never arms (a knob that ships clear of a threshold but whose genome bound sits on the noise floor). Its failure names a mutant index + seed against a fixed `MUTANT_RNG_SEED`, so a red run is a reproducer, not a mystery. |
| `tests/liveness.rs` | `test-harness` (GPU-free) | A scripted agent drives the squad across the dungeon (coverage ≥ 15 distinct cells + no soft-lock); a ~10 s unattended survival run over 20 checkpoints. Also: **Almond Water** seeps and pools on the floor (`peak > 0` after 600 ticks) and a wounded biological flooded with water regains HP in one tick. |

### The G0/G0b/G0c hunt — what it cost, and the one thing that ended it

Kept because the *method* is the reusable part. Three root causes, ten order-dependence bugs, and the class
was only closed by making it **mechanical**:

* **G0** — `laser::fire_laser` drew a shared RNG stream in raw ECS query order.
* **G0b** — `config.ron` held a machine-baked level, so the archive came back empty.
* **G0c** — **`GibKey` was derived from the death origin position**, so the tiebreak for "two chunks at a
  bit-identical spot" was a function of that spot. Two creatures dying on one coordinate minted identical
  keys → `assign_meat_targets` tied → crabs committed to different meat chunks per run.

**The bisect never named G0c.** A session of it narrowed to one tick (1582) and a pair of crabs and stopped
there — a bisect shows where a divergence *surfaced*, not which of a dozen sorts fell through to query
order. `util::sort_total_by_key_at` named it in **one second**, on the first harness run after being wired
up, with the file, the line, and the duplicated key.

Four sites documented the exact trap they then fell into (ORCA, `almond_water_effect`, `smiley_defense`,
`GibKey`). That is why invariant 10 is enforcement and not advice. The two recurring shapes:

1. **A key that is a PREFIX of the value** — `(pos)` where the element is `(pos, payload)`. Coincident actors
   tie and the payload decides something. Crabs `clamp_to_patch`-ed against a wall hold *bit-identical*
   coordinates, so this is routine (measured: 6 fully-tied pairs at one tick).
2. **A tiebreak derived from the tied quantity** — `GibKey`'s mistake. A position-derived key cannot break a
   position tie.

Diagnosis, measurements, and the corrected ruled-out table: `docs/rl/2026-07-16-search-rollout-nondeterminism.md`.

---

## The harness API (`foundation_vs_slop::sim_harness`)

```rust
use foundation_vs_slop::sim_harness::*;

let _serial = serial_guard();                 // hold for the App's whole lifetime — see invariant 4
let cfg = SimConfig::deterministic_core();    // physics OFF → exact-hashable. Or SimConfig::default() (physics ON)
let mut app = build_headless_app(&cfg);       // boots dungeon, spawns, AI, everything — no window
step(&mut app, &cfg, 180);                    // advance 180 fixed ticks (one FixedUpdate each, at speed 1)

let h = snapshot_hash(&mut app);              // u64 (FNV-1a) over every actor's position+health (excludes gib chunks)
let violations = liveness_violations(&mut app); // Vec<String>, empty = healthy (no NaN / bad health / runaway)
```

`SimConfig { fixed_dt: f32 /*1/60*/, speed: f32 /*wall-rate multiplier*/, physics: bool }`.
`Default` = physics **on**; `SimConfig::deterministic_core()` = `{ physics: false, ..default() }`.

**Driving the squad** (headless — bypasses the cursor/window that `command_input` needs):

```rust
issue_squad_order(&mut app, goal_cell);   // build a flow field to `goal`, insert MoveOrder on every unit (false if unreachable)
let cells   = unit_cells(&mut app);       // where the units are now (coverage tracking)
let floors  = floor_cells(&mut app);      // all floor cells (goal source + coverage denominator)
```

---

## The fixed-timestep architecture (where new systems go)

The pinned simulation runs on **`FixedUpdate`** at 60 Hz (frame-rate independent). Cosmetic / FX / input runs
on **`Update`**. When you add a system, decide which:

- **`FixedUpdate`** if it changes pinned state: positions, health, AI decisions, fields, the economy —
  anything another pinned system reads. (AI `AiSet`, movement, combat, laser, fog **LOS**, nest/crab
  economy are all here.)
- **`Update`** if it's cosmetic or per-frame: rendering, materials, animation, audio, camera, diagnostics,
  input reading (`selection::command_input`), the fog *overlay* (`apply_floor_fog`).

Rule of thumb: **if it would appear in `snapshot_hash`, it belongs on `FixedUpdate`.** Ordering constraints
(`.after(AiSet::Think)`, etc.) only work *within one schedule*, so keep interacting systems together.
`Time<Fixed>` is set in `lib::run` (60 Hz) and in the harness (matched to `fixed_dt`).

---

## How to add a test (patterns)

### Pure-logic golden (no App) — the default
Add `#[cfg(test)] mod tests` in the source file (see `ai/utility.rs`). Seed in, assert the exact output.
For a golden over many inputs, use the **print-first** flow: write the test to `println!` the values, run
once with `-- --nocapture`, then paste them in as a `const` and switch to `assert_eq!`. Hash with a
hand-rolled FNV-1a (see `tests/wfc_pin.rs`) — `DefaultHasher` is **not** stable across toolchains/processes.

### Full-sim replay / liveness (harness)
```rust
#![cfg(feature = "test-harness")]
#[test]
fn my_replay() {
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core();          // exact hashing
    let mut a = build_headless_app(&cfg); step(&mut a, &cfg, 180); let ha = snapshot_hash(&mut a); drop(a);
    let mut b = build_headless_app(&cfg); step(&mut b, &cfg, 180);
    assert_eq!(ha, snapshot_hash(&mut b));
}
```
For physics-on behaviour, assert `liveness_violations(&mut app).is_empty()` at checkpoints instead.

### Visual (SSIM)
`assert!(ssim(&golden_gray, &shot_gray, w, h) >= 0.98)`. See `visual_regression.rs` tests.

### Debugging a harness panic
A Bevy "Resource does not exist / Parameter failed validation" with hidden system names? The `test-harness`
feature already enables `bevy/debug`, which prints the real system + resource name.

---

## CI (`.github/workflows/ci.yml`)

- **Hard gate** (`test` job, ubuntu, GPU-free): `cargo test` — the deterministic core must pass on every
  push. Installs Bevy's Linux build deps (alsa/udev/wayland/xkb).
- **Advisory**: `cargo fmt --check` + `cargo clippy` run but **don't block** — the repo predates style
  enforcement (no `rustfmt.toml`, standing clippy lints), so blocking would fail on untouched code.
- **Harness lane** (`harness` job, `continue-on-error`): runs the replay/liveness/SSIM tests. Since the
  harness took `backends: None` it needs no GPU and no lavapipe, so this lane is a candidate for promotion
  to a hard gate.

Pin determinism on a **single** CI target: the RNG is bit-stable, but `f32` gameplay math may diverge across
CPUs/compilers. Treat other platforms with tolerance unless gameplay math moves to fixed-point.

---

## Constraints & "not yet automated"

- **The harness no longer needs a GPU.** It runs with `RenderPlugin { backends: None }` (see "What's in
  the box" §2). The *windowed* game obviously still needs a real backend; only `build_headless_app` omits it.
- **`devshot` can't run inside the harness** — `Screenshot::primary_window()` needs a window, and the
  harness has none. So full SSIM visual-regression runs against the *windowed* game, in the display-gated
  `tests/visual_capture.rs` (`#[ignore]`d, since CI without a display/GPU can't run it):
  `cargo test --features test-harness --test visual_capture -- --ignored`. It launches the game binary,
  drives a `devshot` capture via the `screenshot.request` sentinel, decodes `screenshot.png` with the
  `image` dev-dependency, downscales to a monitor-independent 688×288, and asserts `ssim(shot, golden) ≥
  0.95` (best of a few frames, so a transient VHS-glitch frame can't fail a healthy run) against the
  committed `tests/golden/title_screen.png`. Regenerate the golden after an intentional title-screen art
  change (see that file's module doc). The SSIM oracle math lives in `src/visual_regression.rs` and is
  separately unit-tested.
- **Cross-speed exact equality is not asserted.** The speed knob (`SimConfig.speed`) is deterministic at a
  *fixed* speed, but per-frame `Update` systems that touch the wall clock (hitstop) run once per update
  regardless of sub-step count, so the fixed-step count can differ by one across speeds. Same-seed /
  same-speed is the guarantee.

---

## Quick decision guide

- Testing a **pure function** (a curve, a solver, a hash, geometry, a placement rule)? → in-file
  `#[cfg(test)] mod tests`, `cargo test`. No harness.
- Need to assert **exact same-seed state** of the running game? → harness, `SimConfig::deterministic_core()`,
  `snapshot_hash`, `serial_guard`.
- Checking the game **doesn't crash / soft-lock / NaN** over a long or scripted run? → harness,
  `SimConfig::default()`, `liveness_violations`.
- Comparing **screenshots**? → `visual_regression::ssim` with a tolerance (and the windowed `devshot` rig).
- Added a **system**? → `FixedUpdate` if it touches pinned state, `Update` if cosmetic. If unsure: would it
  show up in `snapshot_hash`?
- Working in **`mycelia`**? → its determinism firewall is a *plugin boundary*, not a property of its systems:
  `MyceliaPlugin` is registered only in `lib::run`, never in `sim_harness`. Most of it is `Update`-only and
  carries no `Health`, but `mycelia::grazing` deliberately runs on `FixedUpdate` and steers crabs (hunger +
  the `MEAT` field). That is pinned state, and it is safe *only* because the harness never registers the
  plugin. Do not move those systems into `crab.rs` — `CrabPlugin` **is** registered in the harness.
- A harness test **flakes**? → you're probably exact-hashing physics-on (use `deterministic_core()`), or
  missing `serial_guard`.

---

## Provenance

The strategy above is derived from the home-still game-testing research corpus:

- **Record-replay + golden-master** as the regression backbone, with determinism as a precondition —
  Politowski et al. (survey), Ostrowski & Aroudj, Bécares et al.
- **Agent exploration** for coverage / soft-lock detection — Lu et al. (Go-Explore), Gordillo et al.,
  Sestini et al. (CCPT), Wuji, Ariyürek et al. A Go-Explore-style navmesh reachability sweep (surfacing
  geometry traps / unreachable WFC regions) is the one RL idea worth borrowing for a solo project; a full
  RL testing agent (Wuji/CCPT-scale) is out of scope.
- **Perceptual glitch detection** for the render layer — Ling et al. (CNN), GlitchBench, RESP; SSIM (Wang
  et al.) as the tolerance oracle.

The documentation itself follows: *know your reader; one source of truth, don't duplicate; document the why,
not just the how* — Ousterhout, *A Philosophy of Software Design* (ch. 12, 16); *The Pragmatic Programmer*
(Tip 13, "Build Documentation In"); Bass et al., *Software Architecture in Practice* (ch. 22).
