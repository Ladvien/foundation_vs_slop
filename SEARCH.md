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

Defaults (from `SearchConfig::default`): 8 generations × batch 4, episode 7200 ticks (≈120 s — a *measured
floor*, below which the authored squad takes no damage on some worlds and the criterion rejects the shipped
game itself), held-in seeds `[0x5C09191, 0xA11CE, 0xBEEF]`, archive resolution 8, RNG seed `0xC0FFEE`,
`--jobs 1` (single-process). `--seeds` accepts hex (`0x5C09191,0xA11CE`).

**`--jobs N` — parallel rollout evaluation.** The search is CPU-bound in `rollout`, which can't be threaded
(the harness pins each process to one thread and holds a process-wide lock for determinism). `--jobs N`
instead spawns `N` `train worker` subprocesses and fans a candidate's `OPPONENTS` independent triples across
them, giving up to a **~3× wall-clock speedup** on a multi-core machine. It is **exact, not approximate**: a
rollout is a pure function of `(brains, world, seed, ticks)` and draws no search RNG, so the archives are
**byte-identical** to `--jobs 1` — pinned by `tests/search_parallel.rs`. The ceiling is `OPPONENTS` (3):
children are sequential (each reads the archive the previous one just mutated), so `--jobs` past 3 buys
nothing. Determinism, reproducibility, and the golden hash are all unaffected.

---

## The full workflow

### 1. Sweep the baseline prior (once)

```bash
cargo run --release --features test-harness --bin train -- prior
```

Writes `assets/config/baseline_prior.ron` — `P(mode | context)` for the game **as shipped**. Surprise is
measured against this and it never moves during a search, so re-sweep it only when you deliberately change
the authored brains. It's validated on load.

### 2. Run the search

Size it first — a default run is multi-hour:

```bash
cargo run --release --features test-harness --bin train -- bench          # projects the budget
# Tiny smoke run (fills a niche or two, proves the pipeline):
cargo run --release --features test-harness --bin train -- evolve3 --generations 1 --batch 1 --ticks 7200 --seeds 0x5C09191,0xA11CE
# A real run (adjust to the budget bench reported):
cargo run --release --features test-harness --bin train -- evolve3
```

Each generation logs `squad / swarm / world` niche counts + QD-scores + `evals / infeasible / failed the
criterion`. Writes `elites_squad.ron`, `elites_swarm.ron`, `elites_world.ron` under `assets/config/` — all
**gitignored**: they are reproducible outputs of `(prior, seed, config)`, not source.

Keep `--ticks` at 7200: below the encounter floor the criterion rejects the shipped game and the search
silently writes empty archives (the `tests/search_calibration.rs` gate exists to catch exactly that).

### 3. Read the elites (before you ship them)

Every elite decodes to the same RON shape you author by hand — that readability **is** the reward-hacking
guard (Skalse et al.). `elites_world.ron` is an `ArchiveDoc`: `resolution`, `coverage` (niches filled),
`qd_score`, and `elites: [ { cell, mean_fear, swarm_aggression, fitness, ai, sim } ]`. The `ai` and `sim`
fields are the decoded config — a **readable diff of world dials** against the shipped values. A human is
meant to open it and be able to refuse what the optimiser found.

The world archive's two axes are player-perceptible: `mean_fear` (how much dread the world induces) ×
`swarm_aggression` (how ferocious a fight). Squad/swarm archives carry `aggression`/`exploration` and
`aggression`/`persistence` respectively.

### 4. Playtest an elite

An elite's `ai` block has the same shape as the `ai_tuning:` slice of `assets/config/config.ron`, and its
`sim` block matches the `sim:` slice. To try a world:

1. Copy a chosen elite's `ai: ( … )` into `config.ron`'s `ai_tuning: ( … )` and its `sim: ( … )` into the
   `sim: ( … )` slice (keep a backup of the shipped `config.ron`).
2. Run the windowed game and watch whether the swarm/squad interaction reads as genuinely different.
3. Screenshot with the in-engine `devshot` (`touch screenshot.request`; see `CLAUDE.md` → "Taking
   screenshots") — **not** the macOS screen-capture tool.

---

## What's evolvable (the config surface)

The world genome is **61 knobs** — the whole data-driven world-dynamics surface, held in two config slices:

- **`ai_tuning:`** (field propagation) — 7 stigmergy channels × {evaporate, diffuse, deposit_radius} + rally.
  Struct + shipped values: `src/ai/tuning.rs`; the slice in `assets/config/config.ron`.
- **`sim:`** (simulation dynamics) — fear gains, deposit strengths, combat, breeding, and boss knobs.
  Struct + shipped values: `src/sim.rs`; the slice in `config.ron`.

The search only ever explores inside a hard per-knob bounds table (`world_genome::BOUNDS`) — evaporation
floored so a field can't saturate, diffusion capped below 1, radii capped so a deposit can't flood the map.
Editing those bounds changes what worlds the search can reach; editing the *shipped* values in `config.ron`
moves the origin the whole search radiates from (and re-run `prior` if you touch the brains).

---

## Operational notes & gotchas

- **`prior` before `evolve`/`evolve3`.** The search loads the frozen prior; running evolve without it errors
  loudly.
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
