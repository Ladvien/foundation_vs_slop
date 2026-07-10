# Testing — strategy, how-to, and reference

The single source of truth for this repo's test system: *why* it's shaped the way it is, *how* to run and
extend it, and a *reference* for the harness API and invariants. Read the strategy section first — one wrong
oracle choice is the difference between a golden regression net and a test that flakes every fifth run.

---

## TL;DR

```bash
cargo test                                                # deterministic core — fast, GPU-free, the CI hard gate
cargo test --features test-harness -- --test-threads=1    # + headless replay / liveness / SSIM (needs a GPU)
```

- **`cargo test`** runs the pure-logic + golden layer (RNG, WFC, utility-AI, ORCA, laser, geometry,
  placement). No GPU, no window, ~instant. **This is what CI blocks on.**
- **`--features test-harness`** additionally boots the *real game* headless and runs replay + liveness +
  visual tests. These need a render device (Metal locally, software Vulkan on CI). They open no window.

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
4. **One App at a time.** Two headless Apps in one process share Bevy's global task pool + the GPU device
   and interfere. Every harness test takes `let _serial = serial_guard();` first and holds it for the App's
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

---

## What's in the box

### 1. Deterministic-core layer (`cargo test`, GPU-free)

Pure functions called directly — **no Bevy `App`**. Fast, deterministic, no GPU. This is the CI hard gate.
See the **Test inventory** below for the full per-module breakdown.

### 2. Headless replay harness (`--features test-harness`, GPU)

`src/sim_harness.rs` boots the **real game plugins** with no window (GPU device present, `WinitPlugin`
disabled). `tests/replay.rs` and `tests/liveness.rs` run against it.

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
| `visual_regression.rs` | The SSIM oracle itself — identical→1.0, tiny perturbation stays > 0.98, structural change scores low, symmetric & deterministic. |
| `placement/manifest.rs` | RON manifest parsing — roles & affordances parse, too-many-tiled is rejected. |
| `placement/solver.rs` | Role→solver routing — routes by candidate role, registration-order-independent, unhandled role → no route, empty candidates → empty success, post-route constraint guard, mixed hard/soft requirements. |
| `placement/furnish.rs` | Room furnishing — typed rooms pick matching kits (a living room gets a seat + screen), same-type rooms can differ, untyped rooms top-up, freestanding constraints are kit-agnostic and spread. |
| `placement/solvers/wfc.rs` | Tiled-WFC placement — stays inside the region rect, no candidates → empty, deterministic under a seed. |
| `placement/solvers/metropolis.rs` | Metropolis layout — objects stay inside & non-overlapping, deterministic under a seed. |
| `placement/solvers/constraint.rs` | Door constraints — exactly one door per room, count places distinct doors, over-count clamped to sites, deterministic selection. |

### `tests/` integration files

| File | Gate | What it does |
|---|---|---|
| `tests/rng_guard.rs` | GPU-free (no feature) | Freezes the exact bit output of every generator — `util` (`next_u32`, `rand01`, `hash01_u32`), `autogib::hash_f32`, and `rng::seeded` ChaCha8 (`raw_u64`, `unit`, `below`). A silent constant change trips here first. |
| `tests/wfc_pin.rs` | GPU-free (no feature) | Golden FNV-1a hash of `wfc::generate` over a 5-seed corpus + in-process reproducibility + the "a floor link only ever joins two floors" invariant. |
| `tests/replay.rs` | `test-harness` (GPU) | Boots the sim; same-seed → identical `snapshot_hash` on the core (`deterministic_core_is_bit_identical`); state evolves; the speed knob is deterministic (does **not** assert cross-speed equality); full-sim liveness. |
| `tests/liveness.rs` | `test-harness` (GPU) | A scripted agent drives the squad across the dungeon (coverage ≥ 15 distinct cells + no soft-lock); a ~10 s unattended survival run over 20 checkpoints. |

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
- **GPU lane** (`harness` job, `continue-on-error`): tries the replay/liveness/SSIM tests under Mesa
  lavapipe (software Vulkan). Best-effort — never reddens the build; locally these run on the real GPU.

Pin determinism on a **single** CI target: the RNG is bit-stable, but `f32` gameplay math may diverge across
CPUs/compilers. Treat other platforms with tolerance unless gameplay math moves to fixed-point.

---

## Constraints & "not yet automated"

- **The harness needs a GPU** (render device). It's headless (no *window*) but not GPU-free. Pure-CPU CI
  runs only the deterministic core; the harness runs locally or on the GPU lane.
- **`devshot` can't run inside the harness** — `Screenshot::primary_window()` needs a window, and the
  harness has none. Full SSIM visual-regression therefore needs the *windowed* game driven by `devshot`
  (`touch screenshot.request`; see `CLAUDE.md` → "Taking screenshots"), plus decoding `screenshot.png`.
  `Cargo.toml` has **no `[dev-dependencies]` section yet**, so the `image` crate needed to decode the PNG is
  absent — adding it is part of building this capture rig. The SSIM oracle itself is built and tested; the
  windowed capture is the remaining piece.
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
