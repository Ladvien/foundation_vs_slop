# G0 — Search rollouts are not reproducible: every search is optimizing noise

**Filed:** 2026-07-16 · **Severity:** blocker for the whole RL/QD roadmap
(`2026-07-14-rlqd-literature-review.md` §9) · **Pre-existing:** yes — verified on stock `HEAD`.

| | Status |
|---|---|
| **G0** — `fire_laser`'s shared aim-scatter draw in query order | **FIXED** (session 2), pinned |
| **G0b** — empty archive from a machine-baked level in `config.ron` | **FIXED** (session 2) |
| **G0c** — residual rollout non-determinism on seed `0xA11CE` | **OPEN** — see §G0c |

> **The headline is not "fixed".** G0 itself is closed and guarded. But session 3 found the guard was passing
> on a **lucky seed**: the search's other held-in world, `0xA11CE`, still splits **3 ways on an idle box**.
> The objective still wobbles. Archives remain untrustworthy for `train apply`.
>
> The good news: G0c reproduces in **~25 s with no load threads**, and `decisions` differs across its
> outcomes (G0's did not) — so it is a *different*, and much cheaper to chase, bug.

## RESOLVED — root cause and fix

**Root cause: `laser::fire_laser` drew the aim-scatter RNG in raw ECS query order.**

```rust
let forward = scatter(*aim, spread, &mut lrng.aim);   // ← inside `for … in &mut shooters`
```

`lrng.aim` is a **shared LCG stream**. The draw ran once per firing unit, in the order the ECS happened to
yield units — an order that is explicitly *not* stable across `App` instances. When two units fired on the
same tick, they could **swap scatter cones**, sending a bolt at a different hostile. The observable effect at
the first divergent tick was exactly `combat.laser_damage` (10.0 HP) moving off the boss and onto another
actor: *one bolt, two targets*.

`LaserRng`'s own doc explains why the `aim` and `friendly` streams are split — so they "never interleave
regardless of system order". That guards *between* systems. Nobody guarded the draw order *within*
`fire_laser`. The same loop even sorts its `NOISE_SQUAD` batch, with a comment noting "the shooter query
order is not stable across App instances" — three lines above the unguarded draw.

**Fix** (`src/laser.rs`): collect the units that will fire, sort by **`SquadMember`** (the stable spawn index
`sim_harness::issue_squad_order` already sorts by, for this exact reason), then draw / spawn / deposit in that
order. Aiming stays in the first pass — each unit writes only its own `AimTarget`, so it is order-independent.
This also canonicalises two more order-dependencies in the same loop: bolt entity-id allocation, and an
**unsorted `THREAT_GUN` deposit** (`Stig::deposit` is a non-associative `f32 +=`, and `drain_deposits` applies
the batch unsorted, so producer push order is load-bearing).

**Proof** — `tests/replay.rs::search_rollouts_are_reproducible_under_load` (12 rollouts @ 7200, synthetic
player, background CPU load):

| | outcomes over 12 reps |
|---|---|
| fix reverted | **4 distinct** → FAIL |
| fix applied | **1** → PASS (and 24/24 identical in a separate run) |

Every golden held (`migrated_defaults…`, `field_passes…`, `authored_world_config_override_is_a_noop`,
`across_many_builds`) — the no-player golden run fires no shots, so the fix is a no-op there. Liveness 8/8.

## WHY THIS HID FOR MONTHS (read this before writing any determinism probe)

1. **`across_many_builds` cannot see it, by construction.** 180 ticks, no synthetic player → the squad idles
   at spawn and **never fires** → `fire_laser`'s draw loop is never exercised with >1 shooter. The strongest
   guard in the suite was blind to the bug.
2. **A quiet machine hides G0 completely.** This is the trap. With the bug *live*, an idle box produced
   **12/12 identical** rollouts in one process and **5/5 identical** across fresh processes. It only split
   into distinct outcomes under CPU load. A clean determinism run on an idle box **proves nothing** — which
   is why the new guard spawns background load threads. Without them the test is decoration.

---

# The original filing, and how it was corrected

Everything below is the investigation as it happened, kept because the *method* is the reusable part — above
all the ruled-out table, which is what stopped the hunt re-treading dead ends, and the two corrections, which
are what stopped it trusting bad measurements. **Where a claim was later disproved it is marked in place
rather than deleted.**

## TL;DR (as originally filed)

Two **identical** evaluations — same genome, world, seed, ticks, **same process** — return different fitness:

```
score 0.02563511  vs  0.023992795      (~6% apart)
```

`evaluate::rollout`'s own doc states the standard this violates:

> *Physics is **off** (`deterministic_core`) … the Avian solver is not bit-reproducible, and **a search whose
> fitness wobbles between identical evaluations is searching noise**.*

That is the current state. `evolve3`, `rl`, `behavior`, `audio` and `poet` all score against a wobbling
objective, so a MAP-Elites cell can be won by evaluation luck rather than by the genome. **Fix this before
trusting any archive, any `train apply`, or any conclusion drawn from a search.**

The red `tests/search_parallel.rs` (`jobs=1 ≡ jobs=N`) is a **symptom, not the disease** — it has been red at
`HEAD` and went unnoticed because it needs a GPU and is therefore not in the GPU-free CI gate.

## Why it matters

- **Archives are partly noise.** Elitism (`>=`) compares fitnesses measured with ~6% jitter; a cell's winner
  can be whoever got the lucky rollout.
- **The common-opponent re-eval is undermined.** `try_insert_with_reeval` exists precisely to compare
  challenger and incumbent under *identical* conditions — but identical conditions no longer imply identical
  scores, so the fix it implements is defeated.
- **`--jobs > 1` can never be proved exact**, because the inline reference itself is not reproducible.
- Every roadmap gap (G1–G8) is closed *through* the search, so all of them inherit this.

## Evidence (all empirical, each probe ~45 s)

| Probe | Result |
|---|---|
| `build_headless_app` + `step(7200)`, twice | **reproducible** — the sim core is fine |
| Each `SimConfig` ingredient (seeded dungeon / `with_world_config` / `with_brains`) | **reproducible** |
| `rollout()` (= `run_episode`: adds the **synthetic player** + recorder), twice | **DIVERGES** |
| 3× `rollout()` in one process | settles into a small set of alternating outcomes |
| Episode length sweep | `180` ok · `600` ok · **`1800` / `3600` / `7200` diverge** |

**Conclusion:** divergence appears only under the **synthetic player**, and only once the episode is long
enough that the squad reaches the crabs and **combat starts** (deaths → gib spawn → crab meat-seeking).
Confirmed pre-existing by stashing all local work and re-running on `HEAD`.

## Already fixed (keep these; they are genuine bugs)

Three real order-dependencies — each one *verbatim* a failure mode named in
`replay::deterministic_core_is_bit_identical_across_many_builds`'s comment ("a non-associative float sum over
an entity list", "a keep-the-first-on-a-tie pick"), which never fired because that guard runs **180 ticks with
no synthetic player**:

1. **`sim_harness::nest_cells`** returned nests in **entity enumeration order** (explicitly unstable across
   `App` instances) and fed `run_episode`'s **stable** `sort_by_key`, so tied nests kept that unstable order
   → the hub tour flipped between runs. *Now sorted canonically by cell.*
2. **`sim_harness::squad_centroid_cell`** summed unit positions with a **non-associative f32 sum in entity
   order** → the centroid could cross a cell boundary and flip the tour's distance keys. *Now sums in
   bit-canonical order (mirrors `coevolve::mean`).*
3. **`sim_harness::issue_squad_order` / `clear_squad_orders`** inserted/removed `MoveOrder` in **raw query
   order**. Component insert/remove is an **archetype move**, and the order of those moves fixes each
   entity's slot in the destination table — i.e. the enumeration order *every later query sees*. This
   laundered the unstable initial order into a persistent one, every dwell/engage cycle. *Now ordered by
   `SquadMember` (the stable spawn index).* Plus `run_episode`'s tour sort is now a **total** order.

Effect: divergence collapsed from arbitrary outcomes to a small alternating set. **It is not gone.**

## Ruled out — do NOT re-investigate

| Hypothesis | Why it's out |
|---|---|
| Multi-threading | `build_headless_app` asserts a 1-thread `ComputeTaskPool`; it would panic, not diverge |
| FP codegen differing between the test and `train` binaries | The rollout lives in the **shared lib**, compiled once and linked into both |
| The bincode IPC wire | Bit-exact and pinned (`test_parallel_wire_roundtrip_is_bit_exact`); also irrelevant — it reproduces **in one process** |
| HashMap-order → float in the fitness | `EpisodeTrace::sequences()` is lookup-only with `Vec`-ordered output; `bayesian_surprise` / `MarkovModel` accumulate **integer** counts into fixed-index arrays and reduce over those indices |
| `FlowField::build` | Pure grid Dijkstra over `Dungeon`, fixed `NEIGHBORS` order, no hash containers |
| Avian moving gibs | `physics: false` skips the plugin entirely (`sim_harness.rs:283`) — gibs never move |
| `crab::assign_meat_targets` | Already exhaustively canonicalised: `gib_snap` by position-bits + `GibKey`, seekers by position-bits + `CrabSeed`, capacity sums bit-canonical |
| The `RoleBrains` / `brains_of` HashMap | `overlay` inserts **by key** and lookups are keyed — order-independent |
| The recorder | `search_calibration::recording_does_not_perturb_the_deterministic_core` |

## Session 2 (2026-07-16, later) — measurements that CORRECT this document

An investigation ran ~40 probe rollouts at the search's real 7200-tick episode. Three of this document's
claims are **wrong** and are corrected below. Everything is measured, not inferred.

> These were the measurements that led to the fix above. Kept because the *method* is the reusable part —
> especially the ruled-out table, which is what stopped the hunt from re-treading dead ends.

### 1. G0 was STILL OPEN in the working tree (the landed fixes did NOT close it)

24 identical `rollout(Authored, None, None, None, 0x5C09191, 7200)` calls in one process alternate between
**exactly two** final snapshots — `decisions = 2009` in every single one:

| snapshot | count |
|---|---|
| `0x8bedc979d819823e` | 15 |
| `0xddeee0048f0d745e` | 9 |

`decisions` being *identical* across both outcomes is the sharpest clue in this document: the AI's discrete
choices are reproducible, so the divergence is **continuous state only** (`Transform`/`Health`) — a float
sum whose order flips, or an RNG draw order, **not** a different decision. Anything that would change a
mode choice is excluded.

Stock `HEAD` (verified in a `git worktree`, tree untouched): 8 rollouts, **1 diverged**
(`0x5cc5d5cf5a8d8e05` / `decisions = 19866` against `0x76cb5aef48297c31` / `19922` for the other 7). So G0
is real at HEAD *and* in the tree.

### 2. **DANGER: the probe is load-sensitive. A quiet machine hides G0 completely.**

The same 7200-tick rollout probe returned **12/12 identical** (and 5/5 identical across fresh processes) on
an idle box, settling on a *third* value (`0xf7dbff955063411e`) — and then alternated between the two values
above when the box was busy. Three distinct outcomes across runs.

**A clean run of any G0 probe proves nothing unless the machine was under load.** This is almost certainly
why G0 reads as "rare" — and it means every G0 measurement in this document (including the ones above)
carries a load caveat. Reproduce under `nproc`-ish busy loops.

### 3. `search_parallel` is red for an UNRELATED reason — the "not the batch emitter" claim is WRONG
###    …and the cause was NOT the cyanide (that diagnosis, below, was wrong — see the correction)

> **CORRECTION (same session, later).** Everything in this section about the Almond Water cyanide being the
> cause is **wrong**, and it is a worked example of measuring on a contaminated baseline. The real cause was
> that `config.ron` held a **machine-baked levels elite** (`train apply --dim levels`, run 08:54 by
> `cargo train all`) in place of the authored level — corridors widened 2→6, topology switched to `Graph`, a
> different seed. The squad fought a different map and lost. Every cell of the poison sweep below was
> measured on that wrong level, which is why "no combination passes both seeds" — the knob was never the
> problem. With the authored level restored and the **poison left untouched at the authored 0.15 / 5.0**,
> both held-in seeds pass:
>
> ```
> seed 0x5c09191: survivors=5  crabs_alive=7  crabs_killed=47  => PASS
> seed 0xa11ce:   survivors=4  crabs_alive=1  crabs_killed=74  => PASS
> ```
>
> The claims "`poison_rate` is inert across 5×" and "the `0x5c09191` squad-wipe persists at poison=0 while
> HEAD passes" were both artefacts of the baked level. The second one was in fact the clue: HEAD passed
> **because HEAD has the authored level**. Kept here, struck through in effect, because the failure mode —
> *sweeping a knob on a baseline you have not verified* — is the reusable lesson.

This document and `tests/search_parallel.rs`'s own header assert the tests are red "ONLY because of G0". They
are not. The assertion that actually fires is **`search_parallel.rs:143` — `assert!(filled > 0, "search
illuminated no niches")`**, which sits *before* every determinism comparison (lines 145-147 never execute).
The archive is **empty**, so the tests never reach the thing they were built to test.

Why it is empty — the authored baseline at 7200 on both held-in seeds:

```
seed 0x5c09191: survivors=1/5  crabs_alive=0  crabs_killed=42  coverage=0.048
seed 0xa11ce:   survivors=1/5  crabs_alive=0  crabs_killed=60  coverage=0.033
CRITERION => Err("swarm went extinct — nothing left to co-adapt against")
```

`crabs_alive == 0` is the **only** failing clause (coverage 0.048 clears `MIN_COVERAGE` 0.02; flatness 0.0017
clears the 0.5 ceiling). Cause: the **new, uncommitted Almond Water cyanide** (`poison_rate: 5.0`,
`belief_poison_frac: 0.15` — *zero* occurrences of `poison`/`belief` in `config.ron` at HEAD) kills the whole
swarm before tick 3600. At 1800 ticks the criterion still passes (`survivors=5, crabs_alive=1`).

**Retuning the poison alone cannot fix it.** A 12-point sweep (`frac × rate`, both seeds, 7200) found **no
passing combination**:

| `belief_poison_frac` | seed `0x5c09191` | seed `0xa11ce` |
|---|---|---|
| 0.15 | swarm extinct | swarm extinct |
| 0.10 | **squad wiped** (33 crabs alive) | swarm extinct |
| 0.06 / 0.03 | **squad wiped** (31-49 alive) | PASS |

Two further findings from that sweep, both open:

- **`poison_rate` is inert.** 1.0 vs 2.5 vs 5.0 give near-identical outcomes at every `frac`; only `frac`
  moves anything. Probably saturation (1 HP/s × 120 s ≫ crab HP), but a 5× knob with no measurable effect
  deserves a look — and it is an **evolvable** gene (`world_genome` bounds `(0.0, 20.0)`), so the world
  search is currently tuning a knob that does nothing.
- **The `0x5c09191` squad-wipe is NOT poison's fault.** With poison fully disabled the tree still gives
  `survivors=0` on that seed, while HEAD **passes** it (`survivors=4, crabs_alive=70`). Some *other*
  uncommitted change (mold couplings? the `config.ron` balance edits?) made the squad lose a fight it used
  to win. Unbisected.

At HEAD, seed `0xa11ce` *already* failed the criterion (`squad was wiped`), so the empty-archive guard was
very likely tripping there too. **G0 and the empty archive are two independent bugs that happen to be red at
the same time.** Fixing G0 will not turn `search_parallel` green on its own.

### Ruled out by measurement this session (do not re-investigate)

| Hypothesis | Why it's out |
|---|---|
| Cold page cache / asset I/O speed | Evicting every asset with `posix_fadvise(DONTNEED)` before each of 4 fresh processes → byte-identical to 4 warm processes |
| FP codegen differing per test binary | No `[profile.release]` LTO section; the lib is a separate rlib that test-file edits never recompile |
| ~~Per-process entropy (ASLR, `HashMap` `RandomState` seeding)~~ **DO NOT TRUST — see below** | ~~5 fresh processes byte-identical; and `RandomState`'s per-instance counter would make consecutive Apps in ONE process differ — 12 consecutive did not~~ |
| std `HashMap`/`HashSet` **iteration** order anywhere in the sim | Partly implied by the row above (now suspect); the *reading* half still stands — `autogib`'s `weld`/`remap`/`assemble_loops::adj` are lookup-only, `parasite`'s `lumped`/`taken`/`furniture_cells` are membership-only |
| ~~Async GLB scene instantiation shifting entity allocation~~ **DO NOT TRUST — see below** | ~~All 14 408 entities exist by tick 1 and the count is constant for 24 ticks, hash identical across 10 reps~~ |

> ### ⚠ Two of the rows above were measured on a QUIET BOX and are therefore worthless
>
> The per-process-entropy and GLB-instantiation rows both "ruled out" their hypothesis with evidence of the
> form *"N runs came back identical"*. **§2 of this same document proves that an idle machine returns 12/12
> identical rollouts with G0 LIVE.** So "identical on a quiet box" is exactly the observation the bug is known
> to produce; it discriminates nothing. Both hypotheses — in particular **App-ordinal / process-history
> state** — are OPEN, not closed.
>
> This is the same error as the cyanide sweep below (measuring on an unverified baseline), in a different
> costume: **an exoneration is only as strong as the condition it was measured under.** Any row in a
> ruled-out table whose evidence is "it didn't reproduce" must record the load condition, or it is not
> evidence. Re-measure both under `nproc`-ish contention before citing them again.
| ORCA neighbour order | Already canonicalised (`squad.rs`, `neighbors.sort_unstable_by_key` by position bits). NOTE: the key is `(x, y)` only — **not a total order**; coincident agents would still permute |

### A/B method note — "remove the suspect" only proves it is not the ONLY source

Two A/Bs were run by disabling a suspect and re-measuring. Read their verdicts carefully:

- **Friendly-fire RNG (`laser.rs`, `friendly_fire_chance = 0`): genuinely not a source.** Arm B produced the
  **same hashes** as arm A — proving friendly fire never fires in this episode at all (no crab is ever shot
  while latched to a unit). Identical output ⇒ the code path is dead here.
- **Aim scatter (`spread = 0`): the verdict "REFUTED" was WRONG.** Arm B produced *different* hashes, i.e. a
  different sim that also diverged. That only shows *another* source exists — it never cleared the aim draw,
  which turned out to be the actual root cause. **When several order-dependencies coexist, removing one
  leaves divergence.** Never read a still-diverging arm as exoneration.

### Same-class leads — NOT the G0 cause (each verified pre-existing; ALL now fixed, session 3)

The G0 fix closed the `fire_laser` instance. These remain: each pushes into a non-associative
`channel[i] += amount * falloff` (`Stig::deposit`) in raw query order, and `drain_deposits` applies the batch
**unsorted**, so producer push order is load-bearing. Most producers *do* canonicalise (by sorting source
positions, or by `CrabSeed`); these do not:

All are real order-dependencies of the documented class — each pushes into a non-associative
`channel[i] += amount * falloff` (`Stig::deposit`) in raw query order, and `drain_deposits` applies the
batch **unsorted**, so producer push order is load-bearing:

**All five are now FIXED (2026-07-16, session 3).** Kept as a record of the class. Fixing them did **not**
close G0c (measured) — they were real bugs of the same family, just not that one.

1. **`update_lasers` (`laser.rs`) — FIXED.** The loop ran in raw bolt query order. Four order-dependencies in
   it, not the three listed here originally:
   - `rand01(&mut lrng.friendly)` — shared stream, **conditional** draw, so bolt order decides which bolt
     consumes which roll. Latent for the authored genome (friendly fire never triggers), armed by any mutant
     that gets a crab latched onto a unit.
   - **`la.entity = Some(laser.shooter)` — a LAST-WRITER-WINS pick, and the one this list missed.** It feeds
     `enemy::smiley_zap`'s instant-kill retaliation: two bolts from different shooters hitting the watcher on
     one tick chose the victim by query order. Unlike friendly fire it needs **no** crab latched to arm —
     only two units shooting the boss at once, i.e. the ordinary case. Verbatim the "keep-the-first-on-a-tie
     pick" trap named in `across_many_builds`'s own comment.
   - the unsorted `THREAT_GUN` deposit, and the `GoreEvent` push, in the same loop that sorts its `noise`.
   *Fix:* `Laser` now carries a monotonic `seq` stamped by `fire_laser` inside its already-`SquadMember`-
   sorted loop; `update_lasers` splits into an order-independent motion pass and a `seq`-ordered effects
   pass. `SquadMember` alone would not do — one unit can have several bolts in flight, so it is not a *total*
   order.
2. **`nest.rs:213`** — `nest_alarm` ALARM deposits. *Fixed:* batched through `sort_deposits`.
3. **`enemy.rs:881`** — SCENT on boss death. *Fixed:* deaths sorted by position bits, SCENT through
   `sort_deposits`.
4. **`squad.rs:416`** — `despawn_dead_units` `GoreEvent` push. *Fixed:* deaths ordered by `SquadMember`.
5. **`squad.rs` ORCA neighbour sort — FIXED.** Keyed on `(pos.x, pos.y)` bits only: **not a total order**, so
   two coincident agents tied and `sort_unstable` fell back to the input order the sort exists to erase. Not
   cosmetic — `new_velocity` runs an *incremental* LP where each line is optimized only against the lines
   before it, and the index of the first infeasible line becomes `linear_program3`'s `begin_line`; reorder
   the constraints and the velocity, hence `Transform`, hence `snapshot_hash`, can move. Coincident positions
   are reachable (units spawn on cell centres; `resolve_move` clamps to identical floats). *Fix:*
   `SquadMember` appended as the tiebreak, mirroring `crab.rs`'s `Seed`/`GibKey` sorts. `orca::Agent` stays a
   pure-math type with no identity field — the key rides beside it in `squad.rs`.

Not the cause: `spawn_fragments`' velocity/spin seeding from `drain_gore`'s shared `Local<u32>` counter
(`gore.rs:865`). Under `deterministic_core` physics is off so gibs never move, and they carry no `Health`, so
they are absent from `snapshot_hash` (TESTING.md invariant 2 is correct). Still a real same-class bug for the
physics-on path.

### The guard this needed — BUILT

`across_many_builds` misses G0 by construction (180 ticks, no synthetic player). Its companion had to run the
**synthetic player at 7200 ticks** under load, and must **not** take `serial_guard()` (`run_episode` acquires
it internally and `HARNESS_LOCK` is non-reentrant — same trap as `a_mutated_audio_config_changes_the_sim`).
Now shipped as `replay::search_rollouts_are_reproducible_under_load`: 12 reps with 8 background load threads.
Reverting the `fire_laser` fix reds it with 4 distinct outcomes.

---

## Reproduce in ~45 s (instead of the 25-minute `search_parallel`)

`tests/determinism_probe.rs` (temporary; `#![cfg(feature = "test-harness")]`):

```rust
use foundation_vs_slop::squad_ai::coevolve::{brains_of, SquadGenome, SwarmGenome, Templates};
use foundation_vs_slop::squad_ai::evaluate::rollout;
use foundation_vs_slop::squad_ai::world_genome;

#[test]
fn three_identical_rollouts() {
    let t = Templates::authored();
    let (squad, swarm) = (SquadGenome::authored(&t), SwarmGenome::authored(&t));
    let wc = world_genome::decode(&world_genome::authored()).unwrap();
    for i in 0..3 {
        let r = rollout(brains_of(&t, &squad, &swarm).unwrap(), Some(wc), None, None, 0x5C09191, 7200);
        eprintln!("ROLLOUT {i}: snapshot={:#x} decisions={}", r.snapshot, r.trace.decisions.len());
    }
}
```

`cargo test --release --features test-harness --test determinism_probe -- --nocapture`.
Swap `7200` for a sweep (`180, 600, 1800, 3600, 7200`) to re-derive the threshold. `Rollout::snapshot` is the
final `snapshot_hash`, which is what makes this cheap and precise.

## G0c — `jobs=1` vs `jobs=N` (session 3)

With G0 and G0b fixed, `search_parallel` reaches its fingerprint comparison for the first time and the two
arms disagree (1 elite vs 2, different genomes). **The parallel plumbing was read end-to-end and is clean:**

| Checked | Verdict |
|---|---|
| `WorkerPool::eval` reduction (`parallel.rs:101-148`) | **Correct** — `slots[idx]` is index-addressed, so results are collected in INPUT order |
| `batch_population` Phase 3 (`coevolve.rs:828-855`) | **Correct** — inserts in pinned predraw order |
| Seed derivation | **Symmetric** — every seed is pre-drawn serially in Phase 1 before any fan-out |
| bincode wire / `ModePrior` / `Templates` | **Bit-exact**, doubly pinned; `ModePrior` is integer counts |
| `coevolve::mean` (`:940-945`) | **Canonical** — bit-sorted before summing |

So G0c is **not** a reduction bug. The two real asymmetries are:

1. **Work assignment is a race.** `parallel.rs:105,121` — workers steal jobs off a shared
   `AtomicUsize::fetch_add`, and workers are long-lived (`worker_main` loops; the pool is spawned once per
   search). *Which* process runs a triple, and at *what ordinal* within that process's `App` sequence, is
   decided by OS scheduling. Inline runs every triple sequentially in one process that has also already built
   the `sweep_prior` `App`s. **`jobs=1 ≡ jobs=N` therefore quietly demands that a rollout be a pure function
   of its inputs REGARDLESS of how many `App`s preceded it in that process** — an invariant nothing states.
2. **`jobs=3` IS the load.** Three contending worker processes are precisely the condition §2 says exposes
   this bug class; `jobs=1` is precisely the quiet condition that hides it. **The two arms of the test differ
   in the one variable that gates the bug's visibility**, so `inline ≠ parallel` is fully explained by
   residual rollout non-determinism with zero plumbing faults.

**Amplifier:** `try_insert_with_reeval`'s `s >= challenger_fitness` (`coevolve.rs:395`) is a razor-thin float
tiebreak. 1 ULP flips who owns a cell, which changes the next generation's parent — that is how a last-bit
wobble becomes "1 elite vs 2".

### The diagnostic — and why the obvious one lies

The naive version ("run the inline search twice") **is not valid on a quiet box**: `inline == inline` is
exactly what a live bug produces there. The inline arm must generate background load (the pattern in
`replay::search_rollouts_are_reproducible_under_load`). Run **inline ×2 under load** and **parallel ×2**:

| Outcome | Meaning |
|---|---|
| `inline ≠ inline` | residual rollout non-determinism, plain |
| `inline == inline`, `parallel ≠ parallel` | same class, amplified by the racy assignment |
| both stable, `inline ≠ parallel` | a rollout depends on **process history** (`App` ordinal) — the deep one |

### MEASURED (2026-07-16, session 3)

```text
PRIOR reproducible : false        <- the frozen reference ITSELF wobbles, on an IDLE box
inline==inline     : false
parallel==parallel : false
inline==parallel   : false
criterion-rejects  : inline 14/12, parallel 16/15   (two identical searches, different reject counts)
VERDICT: inline != inline  =>  residual ROLLOUT non-determinism (plain)
```

**G0c is NOT a parallelism bug.** The reduction, the wire, the seed derivation and the batch emitter are all
exonerated — by reading *and* by this measurement. `search_parallel` is a **symptom**. Do not touch
`parallel.rs`.

`sweep_prior` diverging is the clue that cracked it open: it rolls out the **authored** genome, which
`search_rollouts_are_reproducible_under_load` swore was reproducible 12/12. The difference between them is
the **seed** — and the guard tested only one:

| seed | distinct over 8 reps, IDLE box |
|---|---|
| `0x5C09191` (the guard's seed) | **1** — reproducible |
| `0xA11CE` (the other held-in world) | **3** (6 / 1 / 1) — **diverges** |

**The G0 guard was green on a lucky seed.** It is now widened to both held-in worlds (mirroring
`search_parallel::SEEDS`) and is consequently **RED** — correctly. A reproducibility guarantee is a property
of the sim, not of one dungeon.

### What this bug is, and how it differs from G0

- **It reproduces on an IDLE box in ~25 s.** No load threads needed — a far better position than G0 ever had.
  The quiet-box caveat does **not** apply to this site.
- **`decisions` DIFFERS** across outcomes (5579 / 5452 / 5273). G0's decision count was *identical* across
  its two outcomes, which is exactly what proved G0 was continuous-state-only. This one perturbs AI mode
  choices, so it is a **different bug**, not G0 resurfacing.
- **Divergence window: 600 → 1800 ticks** (600 clean over 6 reps; 1800 splits two ways, decisions 3932 vs
  3929). The `3600: 1 distinct` row in that sweep is a **false negative** — a ~25% event over 6 reps.
- Why `0xA11CE` and not `0x5C09191`: the worlds fight very differently — `0x5C09191` ends `survivors=5,
  crabs_killed=47`; `0xA11CE` ends `survivors=4, crabs_killed=74`. Whatever the site is, the high-kill world
  reaches it and the low-kill one never does. (Invariant 9 again: coverage of a *system* is not coverage of
  its *contended* path.)

### Ruled out for G0c (session 3 — each checked, not assumed)

| Hypothesis | Why it's out |
|---|---|
| The parallel reduction / batch emitter / wire | **Measured**: `inline != inline` at `jobs=1`. Also read end-to-end — see the table above |
| `ai::utility::decide`'s weighted-random pick | Draws from a *caller-supplied* stream; the only production caller (`brain.rs:279`) passes the **per-entity** `ActiveBehavior.rng`. Every shared-stream caller is `#[cfg(test)]` |
| `audio.rs`'s six `Local<u32>` streams | Every audio system is registered on `Update`, never `FixedUpdate` — they cannot write pinned state |
| `crab_despawn_dead`'s gore push | Already sorted by `CrabSeed` (`crab.rs:1517`) |
| `update_lasers` / ORCA neighbour ties | Both were real bugs of this class and are now **fixed** — and `0xA11CE` still diverges, so neither was G0c |

### Next step

Bisect to the first divergent tick on `0xA11CE` inside **600–1800**. The cheap idle-box reproducer makes this
tractable: run N apps **sequentially** (never concurrently — harness invariant 4), each recording a per-tick
`snapshot_hash` trace, then diff two traces that disagree.

It needs `run_episode`'s real schedule (the synthetic player's hub tour), so add a temporary `trace_every`
hook *beside* it rather than re-deriving the schedule. A probe that drifts from the real schedule reports a
window boundary, not the true first divergence — that mistake already cost a session: an `elapsed = 1` offset
made "tick 3840" look like the answer when the truth was 1858.

## Related

- `tests/search_parallel.rs` — red at HEAD; symptom of this bug. Not in CI (needs a GPU).
- `tests/replay.rs::deterministic_core_is_bit_identical_across_many_builds` — the guard that misses it
  (180 ticks, no synthetic player).
- `src/squad_ai/evaluate.rs::rollout` — the doc comment stating the standard being violated.
