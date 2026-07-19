# Running the offline co-evolutionary search

How to drive the offline search that evolves the game's **squad brains, swarm brains, and world-config**
together, read what it finds, and try a result in-game. This is the *operational* guide — the algorithm,
the genome, and the determinism model are documented at their source (see [Where to read more](#where-to-read-more));
this file is the glue that isn't obvious from any one module.

**What it does, in one breath.** It illuminates three MAP-Elites archives — squad, swarm, and world — each
co-adapting against the other two, all optimising *witnessed learnable-surprise* (`W·S·L`) subject to a hard
minimal criterion. You get three human-readable RON archives of "interesting playstyles / worlds," which a
runtime director could later *select* from. It never trains at runtime; it is an offline illuminator.

**Honest ceiling.** The world population evolves *tuning* — field propagation + the combat/economy/fear/boss
numbers — which is a real dynamics change, but it still cannot invent a mechanic no code path implements. An
elite is a *diff of dials*, not a new rule.

---

## TL;DR

```bash
# Everything runs through the test-harness binary. Run LOCALLY (see Prerequisites), --release for speed.
cargo run --release --features test-harness --bin train -- prior          # 1. freeze the baseline (once)
cargo run --release --features test-harness --bin train -- evolve3 --jobs 3  # 2. co-evolve (parallel); writes 3 archives
$EDITOR assets/config/elites_world.ron                                # 3. READ an elite before shipping it
# 4. copy an elite's `ai:`/`sim:` into config.ron's ai_tuning:/sim: slices, run the windowed game, watch.
```

A full default run is a **multi-hour** job (hundreds of ~12 s headless episodes). `--jobs 3` cuts the
wall-clock ~3× on a multi-core machine with byte-identical results (see `--jobs` below). Start small while
you find your footing — see [the workflow](#the-full-workflow).

---

## Prerequisites

- **Run it locally, on this Mac (or `bmb`), not in CI or a remote sandbox.** Determinism is proven here
  (the deterministic-core golden hash matches), the search is a long offline job, and `f32` gameplay math
  can differ across CPUs — so a search is only reproducible on one machine. See `TESTING.md` for the
  two-altitude determinism model.
- **Build with `--features test-harness`.** The search boots the real game headless (`sim_harness`), which
  is opt-in so it never enters the shipped binary. `--release` is ~an order of magnitude faster per episode.
- **Speed 1 only.** The `SimConfig.speed` knob changes the fixed sub-step count; a search must run at
  speed 1 to stay reproducible.

---

## The `train` binary — subcommands

`cargo run --release --features test-harness --bin train -- <SUBCOMMAND> [flags]`

| Subcommand | What it does | Key flags |
|---|---|---|
| `bench`   | Times build-vs-step per seed and **projects the search budget** (rollouts × wall-clock). Run this first to size a real search. | `--ticks --seeds --speed` |
| `probe`   | Runs the **authored** brains once per seed and prints the raw `EpisodeOutcome` + fitness factors + `minimal_criterion` PASS/FAIL. This is how every threshold is **calibrated from measurement**, never guessed. | `--ticks --seeds` |
| `prior`   | Sweeps the authored brains to build the **frozen baseline expectation** and writes `assets/config/baseline_prior.ron`. **Must run before `evolve`/`evolve3`.** | `--ticks --seeds` |
| `evolve`  | The two-population view: co-evolve and commit the **squad + swarm** archives (the world co-evolves but isn't saved). | `--ticks --seeds --generations --batch --seed --res --jobs` |
| `evolve3` | The full run: commit **all three** archives, including the evolved worlds (`elites_world.ron`). | same as `evolve` |
| `worker`  | *Internal.* A rollout-evaluation subprocess spawned by `--jobs N`; speaks a binary protocol on stdin/stdout, never run by hand. | — |

Defaults are the **CLI's** (`SearchArgs`), not `SearchConfig::default()`'s — `coevo_config` builds the
struct field-by-field from the flags, so the `Default` impl never runs on this path: 30 generations × batch
16, episode **7200 ticks** (≈120 s — a *measured floor*; `SearchConfig`'s doc carries the damage-per-seed
table and the reason a shorter episode thins the archives), archive resolution 8, RNG seed `0xC0FFEE`,
`--jobs 1` (single-process; `train all` instead defaults it to every core).

Held-in seeds are `[0x5C09191, 0x1CE5, 0xD00D]`. **Do not copy that list** — the one definition is
`squad_ai::coevolve::HELD_IN_SEEDS`, and every default reads it. The previous set (`0xA11CE`, `0xBEEF`) was
retired when the mold tipped those worlds into squad wipes, and `0xB0BA` was retired on 2026-07-19 when the
baked audio elite tipped it into a wipe too (see `mold::MoldConfig`'s `Default` and `HELD_IN_SEEDS`); stale
copies of the old list in this file outlived the change long enough to send a later reader re-tuning the
episode floor against a world the search no longer runs. `--seeds` accepts hex (`0x5C09191,0x1CE5`).

**`--jobs N` — parallel rollout evaluation.** The search is CPU-bound in `rollout`, which can't be threaded
(the harness pins each process to one thread and holds a process-wide lock for determinism). `--jobs N`
instead spawns `N` `train worker` subprocesses and fans a candidate's `OPPONENTS` independent triples across
them, giving up to a **~3× wall-clock speedup** on a multi-core machine. It is **exact, not approximate**: a
rollout is a pure function of `(brains, world, seed, ticks)` and draws no search RNG, so the archives are
**byte-identical** to `--jobs 1` — pinned by `tests/search_parallel.rs`. The useful ceiling is `batch ×
OPPONENTS` per population per generation: the batch emitter (`batch_population`) scores a whole batch at
once, so `--jobs` scales to the box — raise `--batch` for more width. Determinism, reproducibility, and the
golden hash are all unaffected.

---

## The full workflow

### 1. Sweep the baseline prior (once)

```bash
cargo run --release --features test-harness --bin train -- prior
```

Writes `assets/config/baseline_prior.ron` — `P(mode | context)` for the game **as shipped**. Surprise is
measured against this and it never moves during a search, so re-sweep it only when you deliberately change
the authored brains. It's validated on load.

### 1b. Or train everything, unattended

```bash
# One command, six phases, every core. Bakes each phase and re-pins goldens as it goes:
nohup cargo train all --repin-goldens --no-progress > train-$(date +%F).log 2>&1 &

# Or search everything and bake nothing (you review + bake by hand afterwards):
nohup cargo train all --no-apply --no-progress > train-$(date +%F).log 2>&1 &
```

`--jobs`/`--islands` default to every logical core; `--ticks` defaults to the measured 7200 floor. One of
`--repin-goldens` / `--no-apply` is **required** — see the gotchas below for which and why. In the morning,
read `BAKES.md` and `git diff assets/config/config.ron`, not the test suite.

Note `--no-apply` costs you composition: with it, every phase searches against the same base config, so
baking all of them afterwards ships a combination no search evaluated together. `--repin-goldens` bakes each
phase as it finishes, so the next phase searches on top of it.

### 2. Run the search

Size it first — a default run is multi-hour:

```bash
cargo run --release --features test-harness --bin train -- bench          # projects the budget
# Tiny smoke run (fills a niche or two, proves the pipeline):
cargo run --release --features test-harness --bin train -- evolve3 --generations 1 --batch 1
# A real run (adjust to the budget bench reported):
cargo run --release --features test-harness --bin train -- evolve3
```

Each generation logs `squad / swarm / world` niche counts + QD-scores + `evals / infeasible / failed the
criterion`. Writes `elites_squad.ron`, `elites_swarm.ron`, `elites_world.ron` under `assets/config/` — all
**gitignored**: they are reproducible outputs of `(prior, seed, config)`, not source.

Keep `--ticks` at 7200 — it is the default, and it is a **measured** floor, not a preference. Re-measured
2026-07-19 after the audio elite was baked into `config.ron` (`train probe --ticks N`, authored brains,
damage taken per held-in seed; every cell is 5/5 survivors with the swarm alive):

| seed | 1800 | 3600 | 5400 | 7200 |
|---|---|---|---|---|
| `0x5C09191` | 119 | 119 | 238 | 238 |
| `0x1CE5` | 0\* | 0\* | 0\* | 31 |
| `0xD00D` | 0\* | 77 | 157 | 205 |

\* under half a hit point (`probe` prints `{:.0}`); at that episode length the cell would fail the
`unit_damage_taken > 0.0` clause — which is why the floor sits at 7200. At 7200 every cell passes
`minimal_criterion` with margin (probe reports "all seeds admitted"). Below 7200 it does not: `0x1CE5` is the
binding world — it takes no measurable damage until 7200 (0 → 31), so a candidate slightly less aggressive
than the authored brain is rejected, the admitted fraction collapses, and the archives come back thin.
Replayability spread across the three seeds: 0.078 / 0.071 / 0.067 / 0.054.

`tests/search_calibration.rs` gates the absolute version of this failure. **Re-measure after anything that
moves the deterministic trajectory** — these numbers are a snapshot, and the last one went stale silently.

### 3. Read the elites (before you ship them)

Every elite decodes to the same RON shape you author by hand — that readability **is** the reward-hacking
guard (Skalse et al.). `elites_world.ron` is an `ArchiveDoc`: `resolution`, `coverage` (niches filled),
`qd_score`, and `elites: [ { cell, total_deaths, total_lives, fitness, ai, sim, mold, almond, lighting } ]`.
Those five slices are the decoded config — a **readable diff of world dials** against the shipped values. A
human is meant to open it and be able to refuse what the optimiser found.

**The diff must carry every slice the rollout scored.** `mold` and `almond` were missing here for 23 of the
genome's knobs: the search evaluated them and the archive dropped them on write, so those dials could never
ship and an elite's reported fitness was not reproducible from the config `train apply` baked. If you add a
member to `WorldConfig`, it must reach `WorldEliteDoc`, `elite_overlay::WorldEntry`, `apply_dim`, and
`train apply`'s splice — all four, or the guard above is a fiction.

The world archive's two axes are `total_deaths` × `total_lives` (normalised cross-species deaths and lives —
`world_descriptor`). Squad/swarm archives carry `aggression`/`exploration` and `aggression`/`persistence`
respectively.

### 4. Playtest an elite

Don't hand-copy blocks — an elite is five slices now, and copying two of them is how you end up playtesting
a world the search never scored. Use the runtime overlay, which applies exactly what `train apply` bakes:

```bash
FVS_WORLD_ELITE=assets/config/elites_world.ron cargo run --release        # best-fitness elite
FVS_WORLD_ELITE=assets/config/elites_world.ron#0,7 cargo run --release    # a specific cell
```

1. Run the windowed game and watch whether the swarm/squad interaction reads as genuinely different.
2. Screenshot with the in-engine `devshot` (`touch screenshot.request`; see `CLAUDE.md` → "Taking
   screenshots") — **not** the macOS screen-capture tool.
3. If you want it permanently: `cargo train apply world assets/config/elites_world.ron` (see
   `src/elite_overlay.rs` for the overlay, `train apply` for the bake — both go through one `apply_dim`).

---

## What's evolvable (the config surface)

The world genome is **`world_genome::N` knobs** (104 at the time of writing — read the const, don't trust
this number) across five config slices. `WorldConfig` is the authoritative list:

- **`ai_tuning:`** (field propagation) — 8 stigmergy channels × {evaporate, diffuse, deposit_radius} + rally.
  Struct + shipped values: `src/ai/tuning.rs`; the slice in `assets/config/config.ron`.
- **`sim:`** (simulation dynamics) — fear gains, deposit strengths, combat, breeding, boss, and the SCP-150
  parasite (population/gait/gestation/brood/host-manipulation). Struct + shipped values: `src/sim.rs`; the
  slice in `config.ron`.
- **`mold:`** — the reaction-diffusion mold's dynamics + its gameplay couplings (light dimming, LOS
  occlusion, almond-water seep). `src/mold.rs`.
- **`almond_water:`** — the evolvable subset only (`AlmondWaterDynamics`: seep/heal/poison/belief); the
  structural and visual knobs stay authored. `src/almond_water/mod.rs`.
- **`lighting:`** — the evolvable subset only (`LightingDynamics`: `field_intensity` + `photophobic_gain`).
  Every other light knob is visual, or provably inert in a headless rollout — see `LightingDynamics`'s doc,
  which records why each excluded knob is excluded. `src/light.rs`.

**Not evolved by any genome:** `gore`, `impact_fx`, `vhs`, `dialogue`. The first is mostly cosmetic (its
`gib_*` knobs feed only Avian components, which are inert in a physics-off rollout); the rest are
presentation.

The search only ever explores inside a hard per-knob bounds table (`world_genome::BOUNDS`) — evaporation
floored so a field can't saturate, diffusion capped below 1, radii capped so a deposit can't flood the map.
Editing those bounds changes what worlds the search can reach; editing the *shipped* values in `config.ron`
moves the origin the whole search radiates from (and re-run `prior` if you touch the brains).

---

## Operational notes & gotchas

- **`prior` before `evolve`/`evolve3`.** The search loads the frozen prior; running evolve without it errors
  loudly.
- **`train all` covers all four bakeable dimensions**: `prior → levels → audio → behavior → evolve3 → rl`.
  It used to skip `behavior` (89 knobs of `BehaviorTuning`), so "retrain everything" reached 3 of the 4
  members of `elite_overlay::Dim` and the fourth was hand-run only. `behavior` sits before `evolve3` on
  purpose — it tunes the base the squad brains run on, so the co-evolution should radiate from the tuned base
  rather than have it shift underneath the archives afterwards. (`rl` is correctly never baked: a
  `NeuralPolicy` has no config slice and ships via `FVS_POLICY_ELITE`.)
- **The prior is re-swept after every bake — automatically.** `bake_winner` re-runs `prior` immediately after
  each `apply_archive`, and `ensure_prior_fresh` re-sweeps whenever `config.ron` is newer than the prior. So
  "baking a phase stales the prior the next phase measures surprise against" is handled, and it is why phase
  ordering is safe. `sweep_prior`'s no-drift rule ("a reference that drifted with the population would make
  'surprising' mean only 'different from last generation'") is about drift *within* a search, not across a
  deliberate re-baseline.
- **`train all` refuses to start without a golden decision.** Baking a real elite MOVES the deterministic
  goldens — that is what a successful search means — and a per-bake default aborts on drift so a human decides
  (`apply_archive` step 4). Unattended, that would burn hours and die on the first phase, so `all` fails in
  the first second unless you pass `--repin-goldens` (bake and re-pin as you go) or `--no-apply` (search only,
  bake nothing). Fail fast, not fail late.
- **What `--repin-goldens` actually costs you.** Not determinism: `recompute_goldens_stable` still requires
  every repeated measurement to agree, so a nondeterministic core reds regardless. What you give up is *change
  detection* — and the trap is that a green `cargo test --features test-harness` afterwards proves **nothing**
  about the training, because the goldens were just set to whatever the code emits. It passes by construction.
  Review `git diff assets/config/config.ron` and `BAKES.md`; never the checkmark.
- **`BAKES.md` is the review trail.** Append-only, tracked, one record per bake: which elite (cell + fitness),
  which archive, where its snapshot was kept, and how far each golden moved. Git already holds the baked
  *values* (`git log -p assets/config/config.ron`) and the goldens (`git log -p tests/replay.rs`); the ledger
  adds what git cannot — which elite caused a move (the archives are gitignored and the next run overwrites
  them, so `assets/config/bake_history/` holds the only surviving copy), and per-phase attribution inside one
  `train all` run that git otherwise collapses into a single diff.
- **`evolve3` is not "the 3-system search".** The `3` counts archives committed (squad + swarm + world);
  `evolve` runs the *identical* three-way co-evolution and just doesn't save the world archive.
- **Known wart: a `levels` bake widens ~5 f64 config floats** (`1.2` → `1.2000000476837158`). `WfcWeights` /
  `room_types[].weight` are `f64` (`dungeon.rs:239`) but genomes are `f32`, so the round-trip re-prints them
  at f32 precision. A ~4e-8 relative change, idempotent, and it leaves the goldens alone — but it is noise in
  the diff you are supposed to review. The fix belongs in `scalar_eq` (compare floats at f32 resolution, which
  is all the search can express) and must not disturb its integer path, where `0x5C09191 == 96506257` matters
  and f32's 24-bit mantissa would collide distinct u64s.
- **Calibrate, never guess.** If you change gameplay, re-run `probe` on the shipped config and check the
  printed `EpisodeOutcome` (coverage, damage, `field-sanity` peak/flatness) still clears `minimal_criterion`.
  `search_calibration` is the gate that the shipped brains still produce a real encounter on every world.
- **Reproducible from one `u64`.** A whole run is seeded (`--seed`); same `(prior, seed, config, ticks,
  seeds)` → same archives. The Phase-5 common-opponent re-evaluation draws no fresh RNG, so it doesn't
  perturb this.
- **The archives are outputs, not source** — gitignored, regenerated by a run. Don't hand-edit them; edit
  `config.ron` (the origin) and re-search.
- **A hung headless run** is almost always the harness self-deadlock: never hold `serial_guard()` and then
  call `rollout` (which takes it again) — see `TESTING.md` and the note in `squad_ai::coevolve`.

---

## Where to read more

Per "one source of truth," the details live with the code — this guide points, it doesn't restate:

- **The algorithm** (three-way co-evolution, the frozen prior, the common-opponent re-evaluation): the
  module doc at the top of `src/squad_ai/coevolve.rs`.
- **Fitness** (`W·S·L`, the minimal criterion, the field-sanity clause): `src/squad_ai/surprise.rs`.
- **The genome** (encode/decode, `BOUNDS`, mutation): `src/squad_ai/world_genome.rs`; the brain genome is
  `src/squad_ai/genome.rs`.
- **The harness + determinism model** (physics-off exact hashing, the golden, `serial_guard`): `TESTING.md`
  and `src/sim_harness.rs`.
- **Parallel evaluation** (`--jobs`, the worker pool, why processes not threads, the exactness proof):
  the module doc at the top of `src/squad_ai/parallel.rs` and `tests/search_parallel.rs`.
- **The design record** (why each phase, the honest ceiling): `~/.claude/plans/can-you-please-review-compressed-pinwheel.md`.

---

## Provenance

The search's design is grounded in: MAP-Elites (Mouret & Clune, arXiv:1504.04909); POET's environment/agent
co-evolution and its `EVALUATE_CANDIDATES` common-opponent re-evaluation (Wang et al., arXiv:1901.01753);
multi-agent autocurricula (Baker et al., arXiv:1909.07528); and reward hacking / hard-gate feasibility
(Skalse et al., arXiv:2209.13085). This document follows *know your reader, one source of truth (don't
duplicate), document the non-obvious, keep it high-level and scannable* — Ousterhout, *A Philosophy of
Software Design* (ch. 12–16), the same principles `TESTING.md` cites.
