# Testing — strategy, how-to, and reference

The single source of truth for this repo's test system: *why* it's shaped the way it is, *how* to run and
extend it, and a *reference* for the harness API and invariants. Read the strategy section first — one wrong
oracle choice is the difference between a golden regression net and a test that flakes every fifth run.

---

## TL;DR

```bash
cargo test --workspace                                    # deterministic core — fast, GPU-free, the CI hard gate
cargo test --features test-harness -- --test-threads=1    # + headless replay / liveness / SSIM (GPU-free)
```

- **`cargo test --workspace`** runs the pure-logic + golden layer (RNG, WFC, utility-AI, ORCA, laser, geometry,
  placement). No GPU, no window, ~instant. **This is what CI blocks on.**
- **`--features test-harness`** additionally boots the *real game* headless and runs replay + liveness +
  visual tests. They open no window and need **no GPU**: the harness sets `RenderPlugin` with
  `backends: None`, so every render type is registered but no adapter/device is ever created.

> ### ⚠️ The harness needs `RUST_MIN_STACK=33554432` — set it locally
>
> ```bash
> export RUST_MIN_STACK=33554432   # 32 MiB; or put it in [env] in your own .cargo/config.toml
> ```
>
> CI sets this in `.github/workflows/ci.yml`. **It cannot be committed for you**: the natural home is
> `.cargo/config.toml`, which is deliberately gitignored (a hardcoded absolute path in it once broke CI
> for every commit), so each machine keeps its own.
>
> Why it is needed: the harness pins Bevy's IO task pool to **one** thread — that is what makes system
> order deterministic — and `--test-threads=1` builds ~35 `App`s in a single process, so every asset load
> in the whole suite funnels through one stack. Bevy's glTF loader nests `block_on` per sub-asset, and
> once the SCP-1048 family added four more glbs the default 2 MiB ran out: the suite aborts with
> `thread 'IO Task Pool (0)' has overflowed its stack`, reproducibly, at the same test every run. Bevy
> 0.19's `TaskPoolOptions` exposes no per-pool `stack_size`, so the environment is the only lever.
> Trimming the bear assets (see `BACKLOG.md`) is the real fix.

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
   lifetime.
   **`--test-threads=1` is NOT belt-and-suspenders, and `serial_guard` alone is NOT sufficient across
   binaries.** `--test-threads=1` limits threads *within* one test binary; cargo still runs different
   test **targets in parallel**, and `serial_guard` is a process-local `static`, so two harness `App`s
   from different targets overlap freely. Found 2026-07-26: a batched
   `cargo test --test containment --test nav --test session --test squad` failed
   `session::both_terminal_paths_are_bit_reproducible`, which then passed cleanly when run alone.
   **Re-run a suspicious harness failure as a single target before believing it.** The real fix is a
   cross-process lock (an advisory file lock inside `serial_guard`).
5. **Single-threaded — and pinned in exactly ONE place.** `build_headless_app_unfinished` forces the
   global `ComputeTaskPool` *and* rayon to one thread before any plugin initializes, and **asserts it won
   the init race** (both are process-global `OnceLock`s, so losing the race would silently restore a
   multi-worker pool with no code change). Multithreading, rayon work-stealing, and concurrent Apps each
   break determinism — that's why all three are pinned.
   **Do not add `Schedule::set_executor(SingleThreadedExecutor::new())`.** It would be a *second* mechanism
   for the one guarantee the pool pinning already provides — two paths to the same invariant, which is
   precisely the shape that makes a determinism regression untraceable (which mechanism lapsed?). The pool
   pinning is the single path; it is asserted, and the goldens that depend on it are the evidence.
   **The windowed build is deliberately NOT pinned.** Reproducibility is a property of the *harness*, which
   is the determinism oracle; forcing the shipped game single-threaded would cost frame time to guarantee
   something nothing reads. A windowed session is not a golden and never will be.
   This mirrors the discipline the offline search already documents (`src/squad_ai/parallel.rs`): a rollout
   cannot be parallelised *within* a process, so the only axis of parallelism is **processes** — the same
   split as krABMaga, which parallelises across independent fixed-seed replicates rather than inside one
   seeded trajectory (Antelmi et al. 2024, *Reliable and Efficient Agent-Based Modeling and Simulation*,
   DOI 10.18564/jasss.5300, §2.34).
6. **Fixed dt.** The pinned simulation runs on **`FixedUpdate`** at 60 Hz (`lib.rs`:
   `Time::<Fixed>::from_hz(60.0)`; `AiSet` and field diffusion are registered on `FixedUpdate` in
   `ai/mod.rs`). The harness drives real time by exactly `fixed_dt` per `step` (`TimeUpdateStrategy`), matched
   to the `Time<Fixed>` timestep, so the sim never sees variable pacing — even though `field.rs` still
   integrates by `time.delta_secs()`, that delta is now fixed.
   **`step(n)` advances the fixed schedule `n - 1` times on a fresh `App`, not `n`.** The first
   `app.update()` runs **no** fixed tick: `TimeUpdateStrategy::ManualDuration` routes through
   `Time::<Real>::update_with_duration` → `update_with_instant`, and on the first call `last_update` is
   `None`, so it seeds the clock and returns *without advancing*. Every later update advances by exactly
   `fixed_dt`. This is structural, not a race. Do **not** "fix" `step` — every committed golden is defined
   in terms of `step(n)` (`deterministic_core_is_bit_identical` steps 180 for 179 ticks), so a literal `n`
   would move all of them for no gameplay reason. Any test that asserts on elapsed sim time must account
   for the offset; `session::a_fresh_app_runs_one_fewer_fixed_tick_than_harness_steps` pins it so a Bevy
   upgrade cannot move the ruler silently.
7. **The world is built PER RUN, not at `Startup` (FVS-A-5).** `Dungeon::generate` no longer runs at
   plugin-build time and creatures no longer spawn on `Startup`; both happen on
   `OnEnter(session::RunState::Active)`, ordered by the `RunBuild::{World, Grids, Populate, PostPopulate}`
   chain. Consequences for tests:
   * `build_headless_app` + the first `step` still gives you a populated world — `PostStartup` leaves
     `RunState::Idle`, and the frame's own `StateTransition` (which sits *before* `RunFixedMainLoop`)
     builds it ahead of the first fixed tick. Nothing in an existing test had to change.
   * A system that reads the `Dungeon`, `Stig`, `FogGrid`, `LightField`, `MoldField` or `AlmondWater`
     must be registered in a `RunBuild` phase, **not** `Startup` — those resources do not exist yet when
     `Startup` runs, and Bevy reports it as `Parameter Res<'_, X> failed validation`, not as a missing
     registration.
   * Anything a test spawns that should not survive a run needs `session::run_scoped()`.
   * `session::leaving_and_re_entering_a_run_builds_a_fresh_different_world` is the acceptance test: the
     old world is despawned, a new one is built, and the `RunSeed` has advanced so it is a *different* map.
8. **Test only compiled code.** The crate is a **lib + bin split** — domain modules are declared in
   `src/lib.rs`. `src/combat.rs` and `src/enemies.rs` are shelved (not declared) — do not write tests
   against them. The live enemy path is `enemy.rs` + `crab.rs`.
9. **A determinism probe on an IDLE box proves nothing.** Order-dependence bugs are races: with G0 *live*,
   an idle machine produced 12/12 identical rollouts in one process and 5/5 across fresh processes, and
   only split under CPU load. Any test asserting same-seed reproducibility of a *long, combat-carrying*
   run must generate background load — see `search_rollouts_are_reproducible_under_load`. Without it the
   test is decoration. (The short physics-off goldens don't need this: they're a fixed 180/1800-tick
   trajectory that never enters the racy paths.)
10. **Killing units early races the async fracture bake — settle it, at a FIXED tick.** `autogib::bake_autogib`
   self-gates on the figurine's sub-meshes being present in `Assets<Mesh>`, i.e. on GLB streaming, and its
   own doc states the premise it relies on: *"combat can't start before scenes load, so the bake is a
   completed prerequisite of any death."* True in play; false in a test that kills the squad a second in.
   Measured on adjacent loaded reps of one seed: **45 gib chunks vs 160**, with `gib_hash` splitting one
   tick after the kill while the actor and field hashes still agreed — exactly the silent cascade
   `gib_hash`'s own docs predict (a different `Carryable` then steers `crab::assign_meat_targets`, and the
   bisect lands on the crab, not the cause). Use `sim_harness::step_until_autogib_ready`, **then advance to
   a fixed absolute tick before killing**: waiting alone is not enough, because the wait itself is a
   variable number of ticks, so gating on it and killing immediately compares two different sims (that
   mistake turned 2 distinct results into 5). `session::app_at_stable_kill_point` is the worked example.
11. **Exercise the code you mean to pin.** G0 lived in `laser::fire_laser` and survived for months *behind*
   a 24-build determinism gate, because that gate runs 180 ticks with no synthetic player — the squad idles
   at spawn and never fires. Coverage of a *system* is not coverage of its *contended* path. When a guard
   is meant to pin ordering, check that the scenario actually produces >1 concurrent actor in that code.
12. **Every sort declares its determinism contract — enforced, not commented.** ECS query order is not
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
13. **An exoneration is only as strong as the condition it was measured under.** Corollary of 9, and the
    rule that keeps a ruled-out list honest. "I removed the suspect and it still diverged" does **not** clear
    the suspect — with several order-dependencies live, removing one leaves divergence (the aim-scatter A/B
    was read as REFUTED this way, and it was the actual root cause). "It didn't reproduce over N runs" does
    **not** clear a hypothesis unless the box was loaded. Any row in a ruled-out table must record *how* it
    was measured, or it is not evidence — two rows in the G0 doc had to be struck for exactly this.

---

## What's in the box

### 1. Deterministic-core layer (`cargo test --workspace`, GPU-free)

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

### In-file `#[cfg(test)] mod tests` — pure logic, `cargo test --workspace`

| Module | What it pins |
|---|---|
| `ai/utility.rs` | Response curves (linear/power/logistic/step), dual-utility bucket selection, weighted-random intra-bucket pick, `decide` determinism, safety default. |
| `bevy_orca` (crate) | `new_velocity` ORCA properties — free agent keeps preferred velocity, output clamped to max speed, head-on pairs deflect, speed bound holds across a deterministic 40-config sweep, identical input gives identical output. Moved out of `src/orca.rs`; the sweep's pseudo-positions now come from a hash inlined in the test, so the crate needs no RNG dependency to seed itself. |
| `map_elites/qd.rs` (crate) | The MAP-Elites archive itself — niche binning, elitist replacement, and coverage. **The loop/cmaes/poet tests below used to be `test-harness`-gated and so never ran in the GPU-free hard gate**; as an unconditional crate they do, which is why `cargo test --workspace` went 1579 → 1591 the day the kernel moved. |
| `bevy_stigmergy` (crate) | The stigmergy substrate in cell space — a deposit's linear falloff and its peak at the centre, an empty channel skipped and left empty, evaporation retaining the documented fraction and CLAMPING rather than going negative, diffusion refusing to cross a wall while an isolated cell blends toward itself (the `n > 0` fallback), the hotspot resolving a tie to the LOWEST index (first-max-wins, the property that makes it independent of anything but the grid), the gradient pointing uphill, off-grid reads returning zero rather than panicking, and — pinned because it surprises — a deposit masking its DESTINATION cell but not its path, so a wide radius reaches floor beyond a wall. |
| `bevy_speech_bubbles` (crate) | The dwell model — reading time scales with length, floors at a minimum so a two-word bark is still readable, and empty text still floors rather than vanishing. The rasterizer and billboard are cosmetic (`Update`, `Time<Real>`) and carry no pinned state; `replay.rs`'s `ui_never_leaks_into_deterministic_core` is what would catch an accidental harness registration. |
| `bevy_speech_bubbles/leaf.rs` | The crate boundary, and unusually it forbids TYPE names as well as crate names: `MainCamera`, `SquadMember`, `MenuState`. This crate used to name the game's camera marker, and making the tracking system generic over the marker is what stops it silently breaking in any project with a second 3D camera — naming one again would compile, so the scan is what refuses it. |
| `bevy_light_grid` (crate) | The CPU illuminance grid in cell space — a fixture falling off linearly to exactly zero AT its range, walls casting shadow through a caller-supplied `los` closure, a cone lighting ahead-not-behind while its source cell lights its own footprint, the gradient pointing toward the light and the taxis push taking opposite signs from it (and zero for zero gain), mold dimming darkening a covered cell to `1 - dim` and dragging the peak down with it while `dim = 0` returns early and a SHORT coverage slice reads as 0 rather than panicking, rock cells staying exactly 0.0 so the full grid is foldable, and `phototropic_scale` easing toward target, rate-limiting the step, shrinking back in the dark, clamping `light01`, and never going negative. |
| `bevy_light_grid/leaf.rs` | The crate boundary: `bevy_math` + `bevy_ecs` only. `bevy_render`/`bevy_pbr` are forbidden by name — the day this reaches for one is the day somebody has confused "how bright is this cell for the AI" with "what colour is this pixel". |
| `bevy_stigmergy/leaf.rs` | The crate boundary: `bevy_math` + `rayon` only, no ECS/app/renderer and no game type. A field is reusable only if the CALLER owns the schedule; the moment this takes `bevy_app` it starts making gameplay decisions for its consumer. |
| `map_elites/engine_free.rs` | The crate boundary, enforced, in the shape `emerge-core` established: the manifest allowlist is `serde`, `rand_chacha`, `emerge-core`, and a source scan is the backstop. A search that needed a renderer could not be fanned out across worker subprocesses, and an engine dependency would put thread pools and entropy sources in a path whose whole value is replaying bit-for-bit from one `u64`. |
| `laser.rs` | CPU raycast geometry — sphere/capsule hit & miss, capsule taller than sphere, deterministic. |
| `wfc.rs` | Grid + graph WFC — always-compatible alphabet collapses, contradiction detection, seed determinism, golden draw-order grid, boundary links stay on-grid, degree-cap, isolated nodes, corridor-favouring weights. |
| `dungeon.rs` | Shipped RON parses & generates, per-config determinism, region type tags, room-fit/margin, error-not-panic on bad config, liminality behaviour, Grid vs Graph topology connectivity/non-overlap, and a **golden dungeon snapshot** (`golden_dungeon_snapshot_is_stable`). |
| `geom.rs` | Poisson-disk sampling (determinism, spacing, bounds), Delaunay triangulation (small cases, every-point-a-vertex, determinism), degree-prune. |
| `autogib.rs` | Fracture **topology** — slice/cap geometry, reaches target & deterministic, missing UV/normals synthesized, open-boundary dropped, degenerate plane leaves piece whole. (Counts/structure, never float vertex positions.) |
| `crab.rs` | Crab floor-patch clamp geometry — a crab is never inset into a wall slab (the reported wall-clip bug); walled edges inset, open edges keep full extent. |
| `scp999/movement.rs` | SCP-999's most-anxious target pick — a **total order** (FEAR desc, dist asc, unique `SquadMember` asc): highest fear wins, fear ties break to the nearest, fear+dist ties break to the lowest member, and the winner is **independent of slice/query order** (the determinism-critical property). |
| `almond_water/mod.rs` | The Almond Water field math — `drink` drains exactly & clamps at 0, `tick` accumulates toward the seep/evaporate fixed point & clamps to capacity, diffusion spreads to a neighbour & conserves between two cells, and `validate_config` accepts a valid slice & rejects out-of-range (diffuse/ior/capacity/wounded-frac/negative-seep). |
| `emerge-core/gait.rs` | The shared gait cadence policy (`gait_cycles_per_sec` — one function for the runtime *and* the bench's skate prediction) — a pure clip at its authored speed plays at cadence 1× and so does a 50/50 walk/run mixture (the property that keeps feet planted **through** the crossover), the gait rate clamps at both ends, and no gait weight freezes the phase instead of dividing by zero. |
| `emerge-mapper/anim_cache.rs` | The persisted measurement cache — a measured report round-trips through the RON file and warms a fresh session; a flipped GLB byte, a hand-edited manifest value, or a cache-version bump each drop the entry (exact invalidation via stored-`Rig` equality + file fingerprint, no second hash scheme). |
| `emerge-mapper/vlm.rs` | The VLM labeler's pure core — the prompt carries every live vocab token + note and every mount option with `what` pinned as the first schema key (Tam et al. 2024); the early gate rejects an unknown token NAMING the axis with a did-you-mean and re-sorts validated lists into vocabulary order; every mount discriminant round-trips to `Mount` and the bad ones (unknown class, missing/absurd wall height) reject; fenced JSON tolerated, braceless prose refused; config-missing errs with the tunnel+key remedy and the key never reaches `Debug`. Plus the whole exchange against a **loopback TcpListener stub** (never the network): the reprompt loop feeds the rejection + the model's own prior reply back and accepts the correction as attempt 2 (OVAL-Prompt), a clean first answer is attempt 1, a second rejection is final, and endpoint error envelopes surface verbatim. |
| `emerge-mapper/label_booth.rs` | The two-angle capture rig's pure half — the angles are horizontal mirrors at equal height, framing scales with the measured extent and always looks at the centre (1e-2, because an f32 ulp at the 4 km booth corner is ~0.5 mm), and the job queue is unique-by-target with honest counts. The GPU half (staging, settle, `Screenshot::image` readback) is windowed-only by design — the headless harness pins only that the booth boots inert (no job → no camera, no panic). |
| `emerge-mapper/labels.rs` | The suggestion layer — keys are ids/mesh paths never indices; `apply_fields` REPLACES axis lists (union would make review a merge puzzle) and touches only what the suggestion carries; `needs_labels` is any-missing-judgement (mount deliberately excluded); the batch queue reports progress and cancels clean; the suggestions cache round-trips and drops entries on GLB re-export, retired vocab tokens, version bump, or a vanished target (candidates live by their mesh file, no library row needed); and the `vocab_proposals.ron` merge dedups by (axis, token) with accumulating sighting lists, writes nothing for an empty set, and REFUSES a hand-mangled file rather than overwriting it. |
| `anim/mod.rs` | The cosmetic pose blender — `wrap01` survives NaN/∞, and — through a real `App` on a bare `AnimationPlayer`, no assets — weights ease without jumps and reach the player, both gait clips share one phase and stay paused, the phase holds while idle, and a one-shot restarts on trigger. (The pure cadence math moved to `emerge-core/gait.rs` with the function.) |
| `anim/blend.rs` | The locomotion blend space — directional lobes and the full weight vector are a **partition of unity** everywhere, each cardinal selects its own lobe, `travel_angle` matches Bevy's −Z-forward convention (an axis flip would strafe units the wrong way), the space is **continuous in both speed and angle** (the property the old `RUN_SPEED_FRAC` step function lacked), tiers are monotone, backpedalling while aiming picks the backward clips, and a degenerate angle reads as straight-ahead rather than NaN. |
| `visual_regression.rs` | The SSIM oracle itself — identical→1.0, tiny perturbation stays > 0.98, structural change scores low, symmetric & deterministic. |
| `placement/manifest.rs` | RON manifest parsing — roles & affordances parse, too-many-tiled is rejected. |
| `placement/solver.rs` | Role→solver routing — routes by candidate role, registration-order-independent, unhandled role → no route, empty candidates → empty success, post-route constraint guard, mixed hard/soft requirements. |
| `placement/furnish.rs` | Room furnishing — typed rooms pick matching kits (a living room gets a seat + screen), same-type rooms can differ, untyped rooms top-up, freestanding constraints are kit-agnostic and spread. |
| `placement/solvers/wfc.rs` | Tiled-WFC placement — stays inside the region rect, no candidates → empty, deterministic under a seed. |
| `placement/solvers/metropolis.rs` | Metropolis layout — objects stay inside & non-overlapping, deterministic under a seed. |
| `placement/solvers/constraint.rs` | Door constraints — exactly one door per room, count places distinct doors, over-count clamped to sites, deterministic selection. |
| `squad_ai/surprise.rs` | The behavioural **minimal criterion** — the hard gate deciding whether an episode is worth scoring. Admits a real encounter; rejects the degenerates `fitness` cannot see (wipe, swarm extinction, always-Flee, no coverage). **A completed containment resolves an episode exactly as a kill does** (2026-08-07): the synthetic player holds fire while a capture is under way, so the shipped brains record zero kills on every held-in world — reading the gate as "a crab must have died" made it reject the shipped game and every offline search wrote an empty archive. Pinned in both directions: a capture-only episode is admitted, an attempt that only *broke* is not, and a capture does not excuse a brain that never chose role work (agency is `squad_duty_decisions`, untouched) or that explored nothing. |
| `tests/skip_debt.rs` (`test-harness`) | **The debt tripwires** — one per known-red `--skip` in CI's harness job, each asserting the skip's *reason* still holds, so a red here means "somebody fixed the bug, delete the skip" rather than "something broke". Pure functions over public data, no `App`, no measurable runtime. Currently: the brain-control fraction of a hub cycle (guards `playtest_level` + the candidate-genome test) and the broadcast watch threshold sitting inside the ambient noise floor. |
| `squad_ai/level_genome.rs` | The level genome (dungeon/furniture/mushroom config) — `authored` decodes to the shipped config within f32 precision, mutation stays feasible across 300 draws (every subsystem validator passes), a mutation actually moves a gene, and dropping every room type still keeps ≥1 (+ a matching damp table). |
| `squad_ai/level_quality.rs` | The static level-quality objective — a disconnected level fails the minimal criterion (fitness `None`), one room is fully reachable, infestation & room/corridor split read correctly from the habitat mask, band/reward helpers behave. |
| `squad_ai/level_eval.rs` | The generate-and-measure evaluator (GPU-free) — the shipped level scores in (0,1] and evaluates **reproducibly**; mutated genomes either score or cleanly reject (never panic). Runs the real `Dungeon::generate` / `furnish_all` / `habitat::build` pipeline. |
| `squad_ai/level_search.rs` (`test-harness`) | The level MAP-Elites loop — a short search fills ≥1 archive niche and its archive doc serialises to readable RON. |
| `squad_ai/behavior_genome.rs` | The 89-knob behaviour genome (`behavior:` config subset) — `authored` round-trips exactly, sits inside `BOUNDS` & is feasible, mutation stays feasible across 500 draws, a mutation moves something, wrong-length rejected. |
| `squad_ai/policy.rs` | The policy seam — `Observation` tensor has a stable dim, `UtilityPolicy` matches engine `decide`, `ScriptedPolicy` clamps, and the learned `NeuralPolicy` has a weight count matching its layers + a **deterministic, in-range argmax** `choose` (RNG-independent — exact-hash safe). |
| `squad_ai/policy_genome.rs` | The neuroevolution weight-vector genome — `authored` is deterministic, feasible, and decodes to an MLP; mutation stays in `[-W,W]` & feasible across 200 draws; wrong-length / out-of-range rejected. |
| `map_elites/interest.rs` (crate) | The human-interest proxies — a blowout has ~0 suspense, a back-and-forth fight out-suspenses it, a comeback registers outcome-surprise, an efficient recovery beats a flat walkover on effectance, all terms bounded `[0,1]`. |
| `map_elites/cmaes.rs` (crate) | Separable CMA-ES — **converges on a sphere & contracts sigma** (the correctness check), deterministic from its seed, a short generation is ignored. |
| `map_elites/loops.rs` (crate) | The CMA-ME improvement-emitter loop (`map_elites_cma_loop`) illuminates several archive cells on a synthetic QD problem. |
| `map_elites/poet.rs` (crate) | POET — **open-endedly grows harder niches & more skilled agents** on a synthetic difficulty/skill problem, rejects a hopeless seed pairing loudly, and `learning_progress` tracks recent improvement. |
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
| `crates/emerge-core/tests/metamorphic_clips.rs` | GPU-free (no feature) | **Metamorphic relations for the clip measurer** — the remedy for a measurer with no oracle (Segura et al. 2016; Chen et al. 1998). The committed valkyrie GLB is mutated in memory (`Glb.json`/`bin` are pub) and the outputs must move lawfully: scale ×k → cycle ×k with **bit-identical contact labels**; retime ×k → duration ×k, speed ×1/k, distance fixed; yaw turns the measured travel; mirror negates only sideways travel; reverse negates travel and phase; a doubled cycle doubles per-loop quantities and reads **ambiguous** against itself; a constant placement offset changes nothing; root drift fires the in-place Bad; renamed anchors are loud, never silent; and a biped's feet strike half a cycle apart (the multi-joint machinery's invariant). |
| `tests/valkyrie_asset.rs` | GPU-free (no feature) | **The animation asset contract.** Reads the glb's JSON+BIN chunks directly and pins what `src/squad.rs` bakes in: the wired clip **indices still name the expected clips** (the Mixamo rifle retarget already reordered them once), the gait table's **durations** match the asset to within a frame (they map φ → seek time, so a re-export silently desyncs the feet), every **lower-body mask bone** exists (a missing name shrinks the mask and the aim/fire layer starts posing the legs), the locomotion clips are still authored **in place** (baked root motion would move the character twice), and the manifest's **render scale is still the 1.13** every eyeballed offset (health bar, rig watch) was calibrated to. Instant. |
| `tests/liveness.rs` | `test-harness` (GPU-free) | A scripted agent drives the squad across the dungeon (coverage ≥ 15 distinct cells + no soft-lock); a ~10 s unattended survival run over 20 checkpoints. Also: **Almond Water** seeps and pools on the floor (`peak > 0` after 600 ticks) and a wounded biological flooded with water regains HP in one tick. Also **`every_wired_figurine_keeps_a_well_formed_pose_blend_through_a_live_run`** — the animation blender's integration net: figurines actually stream in and get wired, and every live `PoseBlender` keeps a phase in `[0,1)` and weights that are finite and sum to 1 while units accelerate, strafe, fire and stop. Structural, not pixel-exact — this is the physics-inclusive sim. |
| `tests/scp999.rs` | `test-harness` (GPU-free) | The SCP-999 comfort blob is present in the deterministic core, and an **A/B** (blob placed **on** vs **60 m from** member 0, all else identical) proves a tickle **lowers FEAR faster than proximity-free decay and lifts MORALE** — isolating the calm from natural fear-decay + any self-Ward. Its seek/calm determinism rides `replay.rs` (which now includes SCP-999 in the harness) — the golden hashes did **not** move because the no-player scenario keeps the squad calm, so the comfort blob is inert on hashed state. |

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
`GibKey`). That is why invariant 12 is enforcement and not advice. The two recurring shapes:

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

### Debugging a *link* error that looks like a code bug
**Killing a background cargo process can corrupt `target/`.** Found 2026-07-26: a mass kill of background
builds left ~16 GB of stale artifacts that produced an **undefined-symbol linker error** reading exactly
like a genuine code defect. `cargo clean -p foundation_vs_slop` fixed it.

**The tell is that `cargo check` passes while the link fails.** `check` never links, so a mismatch between
the two is evidence about `target/`, not about the source. Reach for `cargo clean -p` before bisecting a
"regression" that appeared right after you interrupted a build.

---

## CI (`.github/workflows/ci.yml`)

- **Hard gate** (`test` job, ubuntu, GPU-free): `cargo test --workspace` — the deterministic core must pass on every
  push. Installs Bevy's Linux build deps (alsa/udev/wayland/xkb).
- **Advisory**: `cargo fmt --check` + `cargo clippy` run but **don't block** — the repo predates style
  enforcement (no `rustfmt.toml`, standing clippy lints), so blocking would fail on untouched code.
- **Harness lane** (`harness` job) — **still advisory; promotion attempted and reverted 2026-08-05**:
  `cargo test --features test-harness --no-fail-fast -- --test-threads=1`, plus a skip list. 1203 tests over
  37 suites, green on aarch64. Needs no GPU since the harness took `backends: None`.
  - **It is red on x86_64 and has been for a long time.** On `main` it fail-fasted at the *lib* target on
    the SIGMA canary, so `replay.rs` never ran; fixing that and adding `--no-fail-fast` revealed three
    **stale x86_64 goldens** (`migrated_defaults_reproduce_the_shipped_golden_hash`,
    `field_passes_are_bit_identical`, `authored_world_config_override_is_a_noop`). Promoting needs those
    measured and resolved on x86_64 first — **not** skipped; see the long note in `ci.yml`.
  - **`--no-fail-fast` is load-bearing.** `cargo test` stops at the first failing *binary*, so one red suite
    hides every suite after it. That is how three separate defects stayed unknown while this lane was
    running them.
  - **Two kinds of skip, and they are not the same thing.** Four are *slow* (the parallel-search and
    replicate-rollout tests, moved to the nightly job on runtime alone — FVS-J-5). Four are *known-red and
    pre-existing*, each with a `BACKLOG.md` entry carrying its measurements. The second kind is a debt
    list: delete the skip the moment its test is green.
  - It went advisory→gating because staying advisory was measured to cost more than it saved — in one
    session it was concealing five real defects, none of them failing loudly.

Pin determinism on a **single** CI target: the RNG is bit-stable, but `f32` gameplay math may diverge across
CPUs/compilers. Treat other platforms with tolerance unless gameplay math moves to fixed-point.

---

## Constraints & "not yet automated"

- **⚠️ `assets/` is read at RUNTIME, so editing it mid-run invalidates the run.** `config::load_game_config`
  reads `assets/config/config.ron` from disk when each test `App` boots, and `GameConfig` is
  `#[serde(deny_unknown_fields)]`. Add a slice while the suite is in flight and every *later* test panics
  with `Unexpected field named 'x' in GameConfig` — against binaries compiled before the field existed.
  Measured 2026-07-27: 5 failures in `tests/replay.rs` that read exactly like determinism regressions and
  were entirely self-inflicted. **Treat `assets/` as frozen while a suite runs** (same for `site67.ron`,
  the `.wgsl` files, and the elite overlays). Editing `src/` is safe once the *build* has finished, since
  the test binaries are already linked — but not during it.
- **The harness no longer needs a GPU.** It runs with `RenderPlugin { backends: None }` (see "What's in
  the box" §2). The *windowed* game obviously still needs a real backend; only `build_headless_app` omits it.
- **`devshot` can't run inside the harness** — `Screenshot::primary_window()` needs a window, and the
  harness has none. So full SSIM visual-regression runs against the *windowed* game, in the display-gated
  `tests/visual_capture.rs` (`#[ignore]`d, since CI without a display/GPU can't run it):
  `cargo test --features test-harness --test visual_capture -- --ignored title_screen_matches_golden`.
  > ⚠️ **Name the test.** A bare `-- --ignored` also runs `regenerate_golden_from_screenshot`, which is a
  > *re-pinning tool, not a check*: it `expect`s a `screenshot.png` in the crate root and therefore
  > **always fails when you have not just captured one**. The run then reports
  > `test result: FAILED. 1 passed; 1 failed` while the golden it was supposed to check was green —
  > which reads as a visual regression and is not one. (Measured 2026-07-30, doing exactly that.)
  It launches the game binary,
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
