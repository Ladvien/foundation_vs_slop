# Performance review — 2026-07-31

Whole-repo performance review at `fvs-k1-scp610-content-fx` HEAD (`6d7e149`), run as: a literature
pass through home-still → a 2-finder review workflow (user-capped) with per-file adversarial
verification → two auxiliary investigations (trace forensics on `fps_trace.csv`; a vendored
bevy/wgpu source sweep) → fixes applied under `--fix` → full-suite validation. 24 candidates found,
22 survived verification, 15 reported, **5 applied**, 10 deliberately skipped (reasons below).

## Literature anchors

| # | Paper | Where it landed |
|---|---|---|
| L1 | Olsson, Billeter, Assarsson, *Clustered Deferred and Forward Shading*, HPG 2012, doi:10.2312/eggh.hpg12.087-096 | Lights-scaled frame cost: cluster/shadow/extract work per visible light (N-24 amplitude; flicker churn; draw-multiplication findings) |
| L2 | van den Berg, Guy, Lin, Manocha, *Reciprocal n-Body Collision Avoidance*, ISRR 2011, doi:10.1007/978-3-642-19457-3_1 | Candidate-set pruning order in `fire_laser` (cheap gates first) |
| L3 | Teschner et al., *Optimized Spatial Hashing for Collision Detection of Deformable Objects*, VMV 2003 | Grid/broad-phase rationale for the laser target scan |
| L4 | Chilimbi, Hill, Larus, *Cache-Conscious Structure Layout*, PLDI 1999, doi:10.1145/301618.301633 | `LightField` floor-cells restriction; scan-only-what-can-change idiom; per-tile-entity scene-extent finding |
| L5 | Karth, Smith, *WaveFunctionCollapse is Constraint Solving in the Wild*, FDG 2017, doi:10.1145/3102071.3110566 | Worldgen-cost-per-rollout finding (memoization proposal, skipped — oracle trade) |
| L6 | Bugden, Alahmar, *Rust: The Programming Language for Safety and Performance*, arXiv:2206.05503 | Profile findings: the opt-level-1 harness lane; allocation/indirection framing |
| L7 | Spieker, Gotlieb, Marijan, Mossige, *RL for Automatic Test Case Prioritization and Selection in CI*, ISSTA 2017, doi:10.1145/3092703.3092709 | The `a0_` FVS-J-6 fast reproducer (order tests by failure-yield per unit cost) |
| L8 | Such et al., *Deep Neuroevolution*, 2017 (home-still `W2778749116`) | Bake parallelization framing (`--jobs` doc fix; `sweep_prior` finding) |

L4/L6 are full-text/abstract indexed in home-still; L5/L7 are downloaded but await reconversion (the
scribe **olmocr backend is broken** as of today — `workspace/markdown does not exist`; scribe-vlm
works); L1/L2/L3 have no open-access copies resolvable by DOI and are metadata-grounded.

## The 11.7 s oscillation (FVS-N-24)

Fully characterized and re-narrowed; the dated log now lives in **BACKLOG.md § FVS-N-24
(“CHARACTERIZED 2026-07-31 afternoon”)**. One-paragraph version: it is a free-running **11.60 s ± 0.20
wall-clock metronome** delivering a **square ~4.5 s fixed quantum** of degraded frames whose
**amplitude is gated by visible lights** (beat vanishes entirely facing an unlit corridor while the
clock keeps phase); the period is authored nowhere in the repo, the vendored engine has no
multi-second timer, and N-25’s “CPU-bound” only excludes *fragment*-bound — so the prime suspect is
**NVIDIA adaptive GPU clocking**, with the engine’s uncached per-frame shadow/caster machinery as the
amplitude multiplier. The 171 ms “hitch class” mostly dissolved: 20/20 cycles put their worst frame in
the slow phase’s leading edge. **Decisive next test is zero-repo-change**: `nvidia-smi` clock logging
beside a fixed-camera run, then a `-lgc` clock-lock A/B.

## Applied fixes (5 findings, 6 files, ~186 inserted lines, uncommitted)

1. **`src/laser.rs`** — `fire_laser` now runs the front-arc dot rejection *before* the Bresenham LOS
   walk (gates are pure rejections; accepted set unchanged), hoists the shooter’s cell out of the
   candidate loop, and PASS 2 gained a conservative bounding-sphere broad phase (+1.0 m slack) before
   `segment_capsule_hit` [L2][L3]. Hash-neutral by construction and by test.
2. **`src/light.rs`** — `LightField` precomputes `floor_idx` from `Dungeon::floor_cells()`
   (constructor now takes `&Dungeon`); the per-tick peak fold and `apply_mold_dim` scan only floor
   cells. Bit-identical: both writers gate on `is_floor`, rock cells are invariantly 0.0, and
   `fold_fingerprint` still folds the whole grid so the field golden pins that invariant [L4].
3. **`src/fog.rs`** — `apply_floor_fog` repaints only cells `update_los` actually transitioned
   (new `changed_cells` + the existing `cell_tiles` index) instead of walking every floor tile
   (~15–22k `Mut<MeshMaterial3d>` items) on every march step. Cosmetic; outside `snapshot_hash`.
4. **`tests/replay.rs`** — `a0_fvs_j6_mutant3_on_world_0x5c09191_reproduces`: the one known-red cell
   of the mutant guard, pinned (same draw stream, same 8-thread load recipe), named to sort **first**
   so the standing FVS-J-6 red surfaces in minutes, not ~40 [L7]. The full guard runs unchanged.
5. **`src/bin/train.rs`** — the `--jobs` help no longer says “capped useful at OPPONENTS (3)”; the
   batch emitter scales to `batch × OPPONENTS` (48 at default batch). A doc line that was silently
   costing ~8× on hand-run `evolve3` bakes [L8].

Plus: a constraint comment on `HARNESS_LOCK` (see Skipped #4) and the BACKLOG N-24 update.

## Validation

- Fast gate (`cargo test`): green — 947→975 lib tests, determinism lint, WFC pins, asset contracts.
- Dev harness suite: 15 pre-replay binaries green; replay’s first 17 tests green **including all four
  exact-hash goldens** (`deterministic_core_is_bit_identical`, `…_across_many_builds`,
  `field_passes_are_bit_identical`, `migrated_defaults_reproduce_the_shipped_golden_hash`) — the
  hash-neutrality claims of fixes 1–3 are proven, not argued. The `a0_` reproducer passed this run
  (FVS-J-6 detection is ~66%/run at 3 reps — unchanged odds, far earlier placement).
- **Release** golden subset: the same four goldens green at `--release --features test-harness`
  (28.4 s of test time) — the evidence gate for the top test-lane finding (below).
- Full-suite attempts, honestly: attempt 1 died at 13:02 — the kernel **OOM-killed** the under-load
  guard at ~8.8 GB RSS against ~19 GB of ambient services (llama-swap, home-still servers, olmocr
  strays). Attempt 2 (`--no-fail-fast`) re-greened everything up to the same guard, which the kernel
  killed again at 13:39 — this time at only **0.7 GB RSS**: the test binary runs at `oom_score_adj
  200`, making it the box's designated victim whenever ambient memory spikes. The suite continued
  through the tail binaries. Net: **1,050+ tests green on this tree, zero code failures**; the two
  under-load guards (plus the two replay tests sorting after them) remain certified only on the
  pre-edit tree (both passed there, 3,044 s, ~12:50). A guards-only retry wants ≥10 GB available —
  see the `harness-guards-need-memory-headroom` session note. The goldens prove the edits'
  hash-neutrality independently of the guards.

## Skipped findings — and why (user decisions, ranked by expected win)

1. **Harness lane at `--release`** (Cargo.toml:77) — *evidence gate passed* (4/4 goldens agree at
   release; build 8m49s warm). Not flipped unilaterally: it changes the documented workflow mid-flight
   with another session baking tonight. Recommend: one full release pass, then edit TESTING.md’s
   canonical command. Expected ~10–25 min saved per ~62-min pass [L6].
2. **Per-tile dungeon entities → per-region chunk meshes** (dungeon/render.rs:227) — the biggest
   frame-CPU lever (~20–30k entities walked per view per frame, times cascades) but a real project:
   the per-tile fog reveal and knee-wall cutaway need per-chunk redesigns [L4][L1].
3. **Machine-wide harness lock** (sim_harness.rs:24) — implemented, then reverted before compiling:
   `evaluate::rollout` takes `serial_guard` per episode and the bake’s worker processes overlap
   rollouts across processes *on purpose* — a machine-wide lock serializes the 24-worker bake. The
   constraint is now documented on `HARNESS_LOCK`. Right home: a suite-layer lock.
4. **Valkyrie material merge (23→3-4) + scp-150/bear trims** (squad.rs:839, TESTING.md:38) —
   draw-call CPU and App-boot decode wins, but they move the goldens and the `valkyrie_asset.rs`
   contract; needs a deliberate measure-and-re-pin session [L1][L4].
5. **Memoized worldgen in the harness** (sim_harness.rs:389) — byte-identical by construction, but it
   narrows what `deterministic_core_is_bit_identical_across_many_builds` proves; that oracle trade is
   yours to make [L5].
6. **`sweep_prior` parallelization** (artifacts.rs:160) — thread-parallel is forbidden by App
   exclusivity; process-parallel needs a worker-protocol extension. Real (~5–8 min per `train all`),
   not a drive-by [L8].
7. **Furniture load staggering** (furnish.rs:452) — the safe version defers scene-handle attachment,
   not entity spawns (fixture positions feed the pinned light bake); belongs with the N-24 follow-up.
8. **Flicker hum tick rate** (light.rs:1181) — visual taste call; N-26 already banked the big win.
9. **perf_probe self-cost** (perf_probe.rs:275) — it’s the live instrument of an open investigation;
   changing its overhead signature now would decalibrate cross-session comparisons.

## Refuted along the way (so nobody re-finds them)

- “`test-harness` including `bevy/debug` forces cross-universe rebuilds” — refuted by the verifier.
- “`mute_when_background` re-applies volume to every sink every frame” — refuted (write is cheap;
  bevy_audio only reacts to actual volume changes).
- The barrels (252 tris), triangles generally (three independent measurements), host CPU frequency
  scaling, and `MIN_APPEARANCE_RAMP_SECS` — all previously ruled out; stayed ruled out.
