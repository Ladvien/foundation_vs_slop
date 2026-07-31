# Codebase Review: Contradictions, Solutions, and a Better Harness (v2)

*2026-07-31, verified against `f460771`. This file replaces an earlier draft that occupied the same
path: every claim below was re-verified against the tree at HEAD; the draft's five findings are
dispositioned in §2. Research grounding is the home-still corpus; DOIs in §6.*

---

## 0. TL;DR

**The most urgent finding is not a contradiction between rules — it's a live gap in the commit that
landed this morning.** `f460771` wired the 8 gore dials into the world genome end-to-end (BOUNDS,
encode, decode, `WorldConfig`, `apply_dim`, `WorldEliteDoc`, `train apply` splice) **except through the
one seam that matters**: `sim_harness.rs`'s `with_world_config` block never applies `w.gore`, so every
rollout evaluates gore against the authored `config.ron` slice. The search will assign fitness to gore
genes by pure noise — the exact "scored ≠ shipped" divergence `tests/genome_coverage.rs`'s own header
was written against, in the opposite direction. One line fixes it; a seam-guard test prevents the next
one (§3.1).

Beyond that: the real contradictions in this codebase are not the ones the earlier draft named. The
verified list (§1) is led by three: **gore's gib economy is pinned state produced on `Update`**
(frame-rate-dependent sim in the shipped game, invisible to the harness by construction), **a
malformed `user_settings.ron` is still destroyed on frame 1** by the very system whose doc-comment
narrates that bug as fixed, and **the determinism lint cannot see `min_by`/`max_by`** — so its "an
unannotated lethal pick fails this test" guarantee is false, and one `SORT-OK` in the tree is
decorative.

On the harness (§4): two of the earlier draft's five proposals already exist in the tree
(cross-build hash identity; a literature-grounded behavioral-diversity oracle that just lacks a test),
one is a no-op (the genome-coverage test already runs in the hard CI gate), and the two worth building
(a cross-process `serial_guard`; lint/meta-oracle upgrades) are cheap and unblock the repo's actual
worst pain — the 34-minute `replay.rs` lane.

---

## 1. Live contradictions (ranked, all verified at HEAD)

### C1 — CRITICAL: the new gore genome is disconnected from the rollout that scores it

`f460771` added `GoreDynamics` (`src/gore.rs:1687-1730`) and threaded it through the genome
(`world_genome::N` 138→146), the elite overlay, the artifacts doc, and `train apply`. But the single
point where a decoded `WorldConfig` reaches a rollout — `src/sim_harness.rs:346-357` — applies
`ai`, `sim`, `mold`, `almond`, and `lighting`, and **not `gore`**:

```rust
if let Some(w) = cfg.config {
    let mut gc = app.world_mut().resource_mut::<crate::config::GameConfig>();
    gc.ai_tuning = w.ai;
    gc.sim = w.sim;
    gc.mold = w.mold;
    w.almond.apply_to(&mut gc.almond_water);
    w.lighting.apply_to(&mut gc.lighting);
    // MISSING: w.gore.apply_to(&mut gc.gore);
}
```

`GorePlugin` is registered in the headless harness (`src/sim_harness.rs:453`) and reads `gc.gore` at
plugin-build time (`src/gore.rs:596`) — *after* this seam runs — so the one-line apply would work, and
its absence means it currently doesn't. Consequences:

- Every world-genome rollout scores the 8 gore genes against the authored slice. Selection pressure on
  those loci is noise. An elite archived with, say, `max_gibs = 12` was actually evaluated at the
  authored value; `train apply` will then **ship a value that was never tested**.
- `scripts/scheduled_rl_bake.sh` (untracked, ready to schedule) runs `train rl` for ~5 h. Run before
  the fix, that bake burns the compute and pollutes the archive with untested gore loci.
- Secondary: `apply_dim`'s operator-facing confirmation string (`src/elite_overlay.rs:266`) still reads
  `"world (ai_tuning+sim+mold+almond_water+lighting)"` — no `gore` — so the log would misreport what
  was applied even after the seam is fixed.
- Note, not a bug: the local (gitignored) `assets/config/elites_world.ron` predates the required
  `gore` field on `WorldEntry` (no `serde(default)` — deliberate) and will now fail loudly under
  `FVS_WORLD_ELITE`. That is the designed staleness signal, the same `N`-mismatch class BACKLOG.md
  already records for FVS-H-1.

None of the three new tests in `f460771` can see this: they pin encode/decode round-trips and
`apply_dim`, not the harness seam. §3.1 has the guard test that makes this class structural.

### C2 — HIGH: the gib economy is pinned state produced on `Update`

The repo's placement rule (repo `CLAUDE.md`, TESTING.md): *new systems go on `FixedUpdate` if they
touch pinned state (would appear in `snapshot_hash`), else `Update`.* All of gore is on `Update`
(`src/gore.rs:606-618`): `drain_gore`, `confine_gibs`, `cap_blood_pools`, `cap_gib_chunks`,
`despawn_gore`. But `drain_gore` mints `Carryable` + `GibKey` (`src/gore.rs:1240-1249`) and the ring
state whose **order decides cap eviction** — exactly what `gib_hash` folds (unsorted, deliberately:
"the order IS the state") — and the consumers are pinned `FixedUpdate` sim: `assign_meat_targets` /
`carry_gibs` (`src/crab/mod.rs:269-301`) route the crab forage economy, which writes `Drives` and
ultimately `Health`/`Transform` — `snapshot_hash` state.

The harness can't catch this by construction: `step()` drives one frame per fixed tick, so frame- and
tick-indexing coincide and every determinism test passes. In the shipped game the ratio floats with
frame rate, so **gib spawn/eviction timing relative to sim ticks — and through it the crab economy —
is a function of FPS**. This is the Kato et al. (2026) Observation 2 trap in exact miniature: a test
environment that pins away the variability the deployed system actually has. Mycelia got the same
call right and documented it (`src/mycelia/grazing.rs:62-71` on `FixedUpdate`;
`src/mycelia/mod.rs:1016-1018` names grazing "the sole part of this module that touches pinned
state"). Gore is the hole in that discipline.

Fix shape (§3.2): split the plugin's system set — the gib-economy systems (`drain_gore` gib/meat arm,
`confine_gibs`, `cap_gib_chunks`, `despawn_gore` for `Carryable` bodies) move to `FixedUpdate`; the
purely cosmetic set (droplets, blood pools, fog hiding, viscera-without-`GibKey`) stays on `Update`.
**This will likely move the replay goldens — measure, don't predict** (the repo has been burned in
both directions on golden predictions; FVS-N-13), and re-pin via the existing `--repin-goldens`
machinery.

### C3 — HIGH: a malformed `user_settings.ron` is destroyed on frame 1, per the exact mechanism its own doc-comment declares fixed

`load_or_seed` (`src/settings.rs:143-149`): parse failure → `warn!` → `UserSettings::default()`.
`SettingsPlugin::build` (`src/settings.rs:113-123`) inserts the (default) resources and registers
`autosave_on_change` on `Update` with no guard. Bevy change detection reports a newly inserted
resource as changed on a system's first run, so **frame 1 calls `write_settings` and atomically
replaces the player's file with defaults**. One typo in a hand-edited settings file destroys the whole
file permanently, with only a log line.

The doc-comment directly above (`src/settings.rs:160-174`) narrates this bug class as fixed — and even
identifies the trigger: *"`KeyBindings` is deliberately not a trigger here: it changes on insert at
startup, which is what fired the destructive write."* The fix removed one of four insert-at-startup
triggers; `HudSettings`, `AccessibilitySettings`, and `InputSettings` still fire the same way. The
user-global rule ("fail loudly — do not write a degraded substitute to storage") is violated on the
one file that is genuinely user data.

### C4 — HIGH: the determinism lint's coverage claim is false for `min_by`/`max_by`, and the tree already leans on the gap

`tests/determinism_lint.rs:82-87` matches six `.sort*` spellings only. Its contract (and repo
`CLAUDE.md`) claims coverage of "a lethal pick" — but a nearest-pick written as `.min_by` over a
`Query` is that shape and is invisible. **15 `min_by`/`max_by` sites exist in `src/`**, unaudited.
Two are load-bearing evidence:

- `src/selection.rs:449-459` — a `SORT-OK:` comment sits above a `.min_by` click-pick. The lint never
  reads it (wrong construct, and outside the 4-line lookback): **the annotation is decorative.** Worse,
  its justification — a distance tie "means two operatives occupy one point — impossible under ORCA" —
  is the argument shape the lint's own header debunks (wall-clamped agents hold bit-identical
  coordinates; `src/crab/foraging.rs` documents the same scenario for crabs).
- `src/selection.rs:885-892` — the containment-throw origin: nearest living operative by `min_by` over
  a `Query<(&Transform, &Health)>`, no annotation at all. A tie hands the throw origin — and therefore
  containment reach, a sim outcome — to ECS iteration order.

This is precisely Barr et al.'s (2014) warning about trusting an oracle's *claimed* scope: the lint's
failure message ("An unannotated raw sort fails this test. That is the point") asserts a totality the
matcher does not deliver.

### C5 — MED-HIGH: `genome_coverage` certifies slice *names*, not knob coverage — its "0 gaps" is name-deep

`tests/genome_coverage.rs` compares the top-level keys of `config.ron` against a hand-maintained
ledger and (as of `f460771`) asserts `KNOWN_GAPS == 0`. But the ledger row for `placement` credits
"furniture density + Metropolis weights — `level_genome`" while `level_genome::decode` evolves **1 of
15 `MetropolisWeights` fields** (`coherence`; `src/squad_ai/level_genome.rs:420`) — the audit verdict
in BACKLOG.md FVS-I-8 is literally "Zero of 15 move an axis." Same pattern: `PerceptionTuning`
evolves 2 of 13 fields (FVS-I-9); the biome genes exist but are invisible to the 2-axis level
descriptor (FVS-N-21). These three are *known and tracked* — the contradiction is that the ratchet
now reads "0 gaps" at a granularity that cannot see them, which is how the next partial wiring
(e.g. C1) sails through. The PCG-book warning applies here twice over: a coverage metric correlated
with what the ledger *says* rather than what the genome *does* "can only ever provide confirmatory
results."

### C6 — MED: FVS-H-8 — the briefing now advertises a distinction that does not exist

The earlier draft flagged the director's silent fallback (FVS-H-7). That shipped fixed on 2026-07-30
(`src/director.rs:353-365`, `src/ui/briefing.rs:56`, test at `:167`). The **live** defect is its
inversion, already tracked as FVS-H-8 (`BACKLOG.md:544`): `pick_next_challenge` writes
`gc.dungeon`/`gc.mycelia`/`gc.placement.*` via `apply_dim`, but `DungeonPlugin` snapshotted
`gc.dungeon` into `DungeonConfigRes` at plugin-build time, so `generate_dungeon` never re-reads it.
The panel therefore labels two identical dungeons `BRANCH UNIVERSE` and `AUTHORED UNIVERSE`. H-7 was
a distinction the player couldn't perceive; H-8 is a perceived distinction that isn't real — a worse
violation of the same "no path the player can't tell they're on" principle the H-7 fix comment
articulates.

### C7 — MED: the panic budget is miscalibrated by ~30% and blind to `assert!`

`tests/panic_budget.rs` pins `BUDGET = 27`, and the count sits flush at 27. But:

- `src/placement/acceptance.rs` (8 sites, the single largest contributor) is **test-only code** —
  gated `#[cfg(test)] mod acceptance;` at the declaration site (`src/placement/mod.rs:16-17`), so it
  never ships. The scanner only recognizes test code via filenames (`tests.rs`, `*_tests.rs`) and
  *inline* `#[cfg(test)]` blocks — a cfg-gate in the parent module is invisible to it. The module
  header's exemption policy ("a test asserting via `unwrap` is expressing an expectation, not
  shipping a crash") is not what the mechanism implements. Real shipped-panic count: **19**.
- `count_panics` (`tests/panic_budget.rs:111-121`) does not count `assert!`/`assert_eq!`/`assert_ne!`,
  which panic identically in release; non-test `assert!` sites exist across `src/` (geom, wfc, orca,
  behavior_tuning, settings, …). The header's "Everything else — the whole simulation — is in the
  budget" overstates.
- Latent: its `//`-comment stripping is the naive `line.find("//")` its sibling lint
  (`determinism_lint.rs:127-147`) carries a 20-line warning against — a `//` inside a string literal
  truncates the scan for that line. No such line exists in `src/` today; the class is documented as
  having already bitten the other lint.

### C8 — MED: `MeatVolumes` is a dead execution path kept alive by a comment that misdescribes it

`compute_meat_volumes` runs on `Update` (`src/gore.rs:609`), fills a resource whose only consumer is
an `AI_DIAG` log line. The spawn path deliberately ignores it for weights — with an excellent,
correct determinism rationale (`src/gore.rs:1210-1222`: async mesh loads would make weight
nondeterministic) — ending `let _ = volumes;` and the claim *"`volumes` stays wired for the
(hash-free) visual scale."* The visual scale is `Transform::from_scale(Vec3::splat(half * 2.0))`
(`src/gore.rs:1254`) and never touches it. Under the one-path rule this is a second path whose
justification is false; delete the resource + system, or make the comment true. Deleting is
recommended — the determinism note already proves weights must never depend on it.

### C9 — LOW-MED: doc/code drift, including in the artist-facing contract

- `docs/artist_guide.md` pins `TILE_SIZE`/`WALL_THICKNESS`/`WALL_HEIGHT`/`DOORWAY_HEIGHT` to
  `src/dungeon.rs:19/:26/:33/:36` — the file no longer exists (now `src/dungeon/mod.rs:20/:27/:34/:42`;
  values still agree). This is the doc read by people who can't check the code.
- `docs/animation.md` cites `src/hair.rs` — now the `src/hair/` module.
- `TESTING.md:150` describes shelved `src/combat.rs`/`src/enemies.rs` — neither exists; two refactors
  stale.
- The earlier draft in this file carried the same disease (`lib.rs:220` for a panic now at
  `src/dungeon/mod.rs:243`; a `Res<T>` note at `lib.rs:245` that lives at `:292`; quotes attributed to
  repo `CLAUDE.md` that are in the user-global `~/.claude/CLAUDE.md`).

### C10 — LOW-MED: the health-bar fill color is duplicated across the Rust/WGSL boundary with no mechanism

`src/ui/theme.rs:226` (`health_fill: Color::srgb(0.80, 0.78, 0.73)`) and
`assets/shaders/health_bar.wgsl:64` (`vec3<f32>(0.80, 0.78, 0.73)`) agree today. The WGSL comment
says "matching `ui::theme`" — comment-only enforcement, in a repo that lints exactly this class
elsewhere. `HealthBarSettings` mirrors the uniform *layout* but carries no fill color, so theme
cannot feed the shader. (This corrects the remembered "palette disagrees across three files" — the
values currently agree; the missing thing is the mechanism.)

---

## 2. Disposition of the earlier draft's five findings

| # | Draft claim | Verdict |
|---|---|---|
| 1 | "One path" vs. two-altitude oracle is an unacknowledged contradiction | **Wrong.** `TESTING.md:42` is literally titled "Strategy: the two-altitude model (read first)" and repo `CLAUDE.md` states the physics-off/exact-hash split inline. Also a category error: the rule governs product execution paths; oracle *selection by determinism class* is not a fallback — liveness is the only correct oracle for a non-reproducible layer (Kato et al. 2026; Patel & Hierons 2017). |
| 2 | Five unevolved systems; fix by un-gating `tests/genome_coverage.rs` | **Mixed; resolution a no-op.** Gore: fixed by `f460771` (but see C1). Swarm cadence: was already evolved — FVS-I-10 closed 2026-07-30 as stale. Metropolis/Perception/biome: real, tracked (FVS-I-8/I-9/N-21), and invisible to the name-level ratchet (C5). The test was never `test-harness`-gated and already runs in the hard CI gate — the proposed fix changes nothing. |
| 3 | Director silently falls back to the authored world | **Already fixed** before the draft was written (2026-07-30; `briefing.rs:56` + test). The draft missed the live successor FVS-H-8 (C6), which inverts the defect. |
| 4 | Mycelia GPU readback breaches the determinism firewall via `FruitBody` `Transform`s (HIGH) | **Wrong on mechanism.** `snapshot_hash` is a *conjunctive* `(&Transform, &Health)` query (`src/sim_harness.rs:669-671`); a `Health`-less `FruitBody` cannot enter the fold. The real cross-edge (grazing on `FixedUpdate` reading fruit `Transform`s) is deliberately firewalled by plugin boundary and documented at `src/mycelia/grazing.rs:15-26`; `MyceliaPlugin` is windowed-only. The one valid residue: **nothing tests that boundary** — see §4.2. Low-Med, not High. |
| 5 | Panic sites violate the no-panic rule | **Substance already managed** by the `panic_budget.rs` downward ratchet (which the draft didn't mention); three of its citations wrong; its proposed "startup panics OK" refinement is **unsafe** — `generate_dungeon` runs `OnEnter(RunState::Active)` per expedition with an advancing `RunSeed`, so a zero-room collapse panic can fire mid-campaign, not just at boot. |

The meta-lesson mirrors the repo's own recorded experience with backlog items: **prose about this
codebase goes stale in days, and a prior session's draft is untrusted input** — its APIs
(`make_app`, `step_until`, `RunSeed`) never existed in this tree at all.

---

## 3. Recommended solutions (priority order)

1. **C1 — one line + one guard test.** Add `w.gore.apply_to(&mut gc.gore);` at
   `src/sim_harness.rs:357` and append `+gore` to the `apply_dim` string
   (`src/elite_overlay.rs:266`). Then the structural fix: a **seam-guard test** — build a
   `WorldConfig` where every slice carries a sentinel value distinguishable from the authored config,
   run it through `SimConfig::with_world_config` → `build_headless_app`, and assert each
   corresponding `GameConfig` field/`apply_to` target actually changed. That test fails today (gore),
   and fails for every future slice someone wires everywhere but the seam. Do this **before** any
   `train rl` bake; `scheduled_rl_bake.sh` should not be scheduled until it lands.
2. **C3 — stop the settings clobber.** Record the load outcome (`Loaded | Malformed`) in a resource;
   `autosave_on_change` refuses to write while `Malformed` and the failure is surfaced loudly
   in-game (not a log line). No `.bak` sidecar — that's the forbidden backup-mode shape; the file
   stays untouched until the player fixes or deletes it. (Exact UX is a design call — the invariant
   that matters: **a parse failure must never lead to a write on a frame the player didn't act.**)
3. **C2 — reschedule the gib economy.** Move the sim-relevant gore systems to `FixedUpdate`; leave
   cosmetics on `Update` (split detailed in C2). Measure golden movement rather than predicting it;
   re-pin via `train … --repin-goldens` if it moves.
4. **C4 — extend the lint; fix the two picks.** Add `.min_by(`/`.max_by(`/`.min_by_key(`/
   `.max_by_key(` to the matcher; require annotations at those sites. Rewrite
   `src/selection.rs:449`'s justification honestly (tie reachable; pick needs a stable total
   tiebreak — `SquadMember` index, never `Entity`) and annotate/tiebreak `:885`.
5. **C5 — make the ledger knob-level.** Per slice, record `(evolved_fields, total_fields, tracking
   item)` and assert both numbers against the structs (a hand-table is fine; it turns silent drift
   into a failing diff). "0 unknown gaps, N known partials" is the honest invariant.
6. **C7 — recalibrate the panic budget.** Rename `acceptance.rs` → `acceptance_tests.rs` (matches the
   existing exemption; zero new lint code), re-pin `BUDGET 27 → 19`; add the `assert!` family to
   `count_panics` and re-pin once more with a comment; replace the naive `//` strip with the
   literal-stripping helper `determinism_lint.rs` already contains (share it via `tests/common/`).
7. **C8 — delete `MeatVolumes`** (resource, system, the two threaded params).
8. **C6 — FVS-H-8** is tracked; the fix direction the backlog already names (make run-build read
   `GameConfig` live, or re-snapshot `DungeonConfigRes` on `OnEnter(RunState::Active)`) also
   future-proofs every other slice the director may dial.
9. **C9 — batch doc fix** (artist_guide line pins, animation.md, TESTING.md:150).
10. **C10 — feed the fill color through `HealthBarSettings`** (add the vec3 to the uniform theme
    already mirrors), or, minimally, a test that extracts the WGSL constant and compares against
    `theme.rs`.

---

## 4. A better harness

What exists is strong: two-altitude oracles (exact hash for the physics-off core; liveness for
physics-on), golden re-pin machinery with an audit trail, source lints in the GPU-free hard gate,
metamorphic pairs (`a_mutated_{world,audio}_config_changes_the_sim` — change must *move* the hash),
and a cross-build identity test (`deterministic_core_is_bit_identical_across_many_builds`,
`tests/replay.rs:733`, N=24 sized to the ~1%/build detection rate). The earlier draft's harness
section should be discarded: its sketches use APIs that don't exist. The real ones:
`build_headless_app(&SimConfig)`, `step(&mut app, &cfg, ticks)`, `snapshot_hash(&mut app)`,
`SimConfig::dungeon_seed`, `HELD_IN_SEEDS`.

### 4.1 Build these, in this order

**(a) Cross-process `serial_guard` — highest leverage.** Today: process-local `static Mutex`
(`src/sim_harness.rs:24-30`), 96 call sites; `cargo test --test a --test b` overlaps binaries — it
already produced one false determinism failure (BACKLOG Push 8), and BACKLOG.md:745 asks for exactly
this. Shape: **keep the `Mutex` as the intra-process layer and nest an advisory `flock` inside it**
— intra-process semantics (including the three sites that *depend on* documented non-reentrancy:
`tests/replay.rs:788`, `tests/search_calibration.rs:41`, the audio-mutation note) stay byte-identical,
and only the cross-binary layer is new. `libc` is already a `test-harness` dependency; lock file
under `std::env::temp_dir()`, not a hardcoded path; `#[cfg]` no-op off unix. The payoff is bigger
than correctness: a cross-process lock is the **prerequisite for running harness targets in parallel
processes**, the named next lever on the 34-minute `replay.rs` / 2-hour lane (FVS-J-5) — the suite's
worst real pain. Ostrowski & Aroudj (2013) is the design precedent: game regression testing without
per-test isolation, coordinated at the infrastructure layer.

**(b) Firewall assertion tests — the ECS read/write-set boundary, made mechanical.** The pattern
exists: `ui_never_leaks_into_deterministic_core` (`tests/replay.rs:916`). Add the missing analogues:
assert the headless app contains no `MyceliaPlugin` state (no `FruitBody`-typed queryable state, no
readback resource), and add the **seam-guard test from §3.1**. Grounding: Tasnim & Zhao (2026) and
Redmond et al. (2025) — determinism in ECS comes from disjoint read/write footprints; a boundary
enforced only by a plugin-registration convention and two comments is exactly the kind that "moving
them one file over would quietly breach" (the grazing module's own words).

**(c) Behavioral-distribution oracle — one test; the library is already written.**
`src/squad_ai/replayability.rs` (`RunSignature` — 6 axes, `spread()`, `replayability_gated()`) *is*
the distribution-based evaluation Kato et al. (2026) prescribe ("use trends and frequencies rather
than single outcomes"), already grounded in expressive-range analysis, already unit-tested — and its
only caller is the print-only `train probe`. Add `behavioral_diversity_over_seeds`: rollouts over
`HELD_IN_SEEDS`, `RunSignature` each, assert `replayability_gated(&sigs) >= FLOOR`. Calibrate `FLOOR`
from `train probe` measurements (risk-based thresholding per Kato; statistical-oracle tolerance per
Guderlei & Mayer 2007 / Patel & Hierons 2017 — this is the *sanctioned* place for a tolerance oracle,
the physics-on altitude, not the exact-hash core). It catches the regression class no hash can: all
seeds converging to one outcome. ~7-8 min for 3 seeds → the nightly lane, not per-PR. The PCG-book
caveat carries over to the existing descriptor-degeneracy watchlist: axes must stay far from genome
inputs or the oracle is confirmatory.

**(d) Meta-oracles — the affordable 90% of "mutation testing."** Two zero-compile-cost tests that
verify the guards are load-bearing (Barr et al. 2014's answer to "who tests the oracle"):
`catch_unwind` around `sort_total_by_key_at` with a deliberately tied key, asserting the panic and
its site-naming message; and a `determinism_lint` self-test that scans a synthetic tree containing an
unannotated sort (and, post-C4, an unannotated `min_by`) and asserts detection — same shape as its
existing `cfg_test_detection_ignores_feature_names`. **Do not** build remove-a-sort-and-rebuild
mutation testing: a full Bevy rebuild per mutant against a target test that is ~30%-flaky-by-design
under load conflicts with the repo's compile-cost rule and buys nothing (a) and (b) don't.

**(e) Perf: a ratchet lane, not a wall-clock golden.** `train bench` (`src/bin/train.rs:1996`)
already measures the right things separately (app build vs. `step` throughput — WFC+placement cost
vs. per-tick cost). A committed wall-clock threshold is a machine-dependent golden in a repo whose CI
is ~3× slower than the dev box and whose culture explicitly rejected tolerance oracles for goldens.
Start non-gating: bench writes `(commit, ticks/s, build_ms)` to a trend file per nightly run;
promote to a gate only as a *ratio* against a same-process reference workload if drift actually
bites. (Kato et al.'s longitudinal-observation direction, applied to performance.)

**(f) The J-6 localizer.** The open CI-contention determinism failure (FVS-J-6) is the one live
determinism bug, its reproducer was deliberately deleted (correctly — 19.5 min/run), and the
replacement ("40 lines of `trace_episode`") was never written. Write it as an `#[ignore]`d test so it
exists the day the failure recurs, costing CI nothing.

### 4.2 Cheap hygiene wins already identified by the tree's own docs

- The bare `cargo test --features test-harness -- --ignored` footgun: `regenerate_golden_from_screenshot`
  runs (and fails) alongside the SSIM check — gate it behind an env var so `--ignored` means "run the
  checks," not "run the re-pin tool."
- `RUST_MIN_STACK` remains manual per-dev (`.cargo/config.toml` deliberately gitignored); the real fix
  is trimming the SCP-1048 bear assets — until then it belongs in the error message the overflow
  produces, if it isn't already.
- The aarch64 golden decision (fixed-point core vs. permanently per-platform goldens) is a **product
  decision** blocking FVS-C-6, not a harness task; no test can make it.

### 4.3 Don't build (from the earlier draft)

- **Replay-portability test** — exists (`tests/replay.rs:733`; gib variant `tests/session.rs:334`). At
  most raise N/ticks if a cumulative-global amplifier (`GibSeq`) is suspected.
- **Distinct-snapshot-hashes-across-seeds diversity oracle** — vacuous: distinct dungeon seeds are
  distinct-by-construction. (c) measures behavior, not seeds.
- **Un-gate `genome_coverage`** — it was never gated; it's in the hard gate today.
- **`/tmp/opencode/harness.lock`**, `make_app`, `step_until`, `RunSeed` — none of these exist.

---

## 5. Summary tables

| # | Contradiction | Severity | Fix |
|---|---|---|---|
| C1 | Gore genome never applied at the rollout seam (`sim_harness.rs:346-357`) | **Critical** | 1 line + seam-guard test, before any bake |
| C2 | Gib economy (pinned state) produced on `Update` | High | Split schedule; measure + re-pin goldens |
| C3 | Malformed `user_settings.ron` clobbered on frame 1 | High | Load-state resource gates autosave; loud surface |
| C4 | Determinism lint blind to `min_by`/`max_by`; decorative SORT-OK | High | Extend matcher; fix 2 selection.rs picks |
| C5 | Genome coverage is name-deep; "0 gaps" overstates | Med-High | Knob-level ledger with counts |
| C6 | FVS-H-8: briefing labels a distinction that doesn't exist | Med (tracked) | Re-read config at run build |
| C7 | Panic budget: 8 phantom test-only sites; `assert!` blind; naive `//` strip | Med | Rename + re-pin 19; extend matcher; share stripper |
| C8 | `MeatVolumes` dead path with false comment | Med | Delete |
| C9 | Doc drift incl. artist contract line-pins | Low-Med | Batch fix |
| C10 | Health-bar color duplicated Rust↔WGSL | Low-Med | Uniform field or extraction test |

| Harness action | Status today | Verdict |
|---|---|---|
| flock-in-Mutex `serial_guard` | asked for by BACKLOG.md:745 | Build first — unblocks parallel lane split |
| Firewall + seam-guard tests | pattern exists (`replay.rs:916`) | Build — makes two conventions mechanical |
| `behavioral_diversity_over_seeds` | library shipped, uncalled by tests | Build — one test + calibrated floor, nightly |
| Guard meta-oracles | partial self-test exists | Build — two cheap tests |
| Perf trend lane | `train bench` exists | Non-gating first; ratio ratchet only if needed |
| J-6 localizer | deleted; bug still open | Rewrite as `#[ignore]`d tool |
| Replay portability, hash-diversity, source-mutation CI, wall-clock goldens | — | Don't build |

---

## 6. References (home-still corpus)

- Kato, Y., Yoshida, N., Makihara, E., & Inoue, K. (2026). *Software Testing Beyond Closed Worlds:
  Open-World Games as an Extreme Case.* `10.48550/arXiv.2604.04047` — non-determinism/unstable-oracle
  observations; distribution-based evaluation; probabilistic oracles with risk-based thresholds;
  state-diversity and transition-coverage metrics.
- Barr, E.T., Harman, M., McMinn, P., Shahbaz, M., & Yoo, S. (2014). *The Oracle Problem in Software
  Testing: A Survey.* `10.1109/tse.2014.2372785` — oracle scope/trust; testing the oracle.
- Guderlei, R. & Mayer, J. (2007). *Statistical Metamorphic Testing.* (via Barr survey) — hypothesis
  tests as oracles for randomized programs; grounds the diversity floor.
- Patel, K. & Hierons, R.M. (2017). *A mapping study on testing non-testable systems.*
  `10.1007/s11219-017-9392-4` — metamorphic heuristic oracles; tolerance for float non-determinism.
- Segura, S., Fraser, G., Sanchez, A.B., & Ruiz-Cortés, A. (2016). *A Survey on Metamorphic Testing.*
  `10.1109/TSE.2016.2532875` — the `a_mutated_*_changes_the_sim` pattern, formalized.
- Ostrowski, M. & Aroudj, S. (2013). *Automated Regression Testing within Video Game Development.*
  `10.7603/s40601-013-0010-4` — regression testing without per-test isolation.
- Mouret, J-B. & Clune, J. (2015). *Illuminating search spaces by mapping elites.*
  `10.48550/arXiv.1504.04909`; Fontaine, M.C. et al. (2020). *Covariance Matrix Adaptation for the
  Rapid Illumination of Behavior Space.* `10.1145/3377930.3390232` — the archive as a
  diverse-by-construction behavioral measurement device.
- Shaker, N., Togelius, J., & Nelson, M.J. — *Procedural Content Generation in Games*, ch. 12; and
  G. Smith, *Game AI Pro 2*, ch. 40 — expressive-range analysis; metrics must be far from generator
  inputs (the confirmatory-metric trap).
- Le Pelletier de Woillemont, P., Labory, R., & Corruble, V. (2022). *Automated Play-Testing through
  RL Based Human-Like Play-Styles Generation.* `10.1609/aiide.v18i1.21958` — population-relative
  summary metrics as behavioral descriptors.
- Bergdahl, J. et al. (2021). *Augmenting Automated Game Testing with Deep RL.*
  `10.48550/arXiv.2103.15819`; Roohi, S. et al. (2021). *Predicting Game Difficulty and Engagement
  Using AI Players.* `10.1145/3474658` — agent-based playtesting; best-case-run metrics.
- Tasnim, A. & Zhao, T. (2026). *The Essence of Entity Component System.* `10.1145/3748522.3779910`;
  Redmond, P. et al. (2025). *Exploring the Theory and Practice of Concurrency in the ECS Pattern.*
  `10.1145/3763050` — read/write-set conflict typing; determinism via disjoint footprints.
- Politowski, C., Petrillo, F., & Guéhéneuc, Y-G. (2021). *A Survey of Video Game Testing.*
  `10.48550/arXiv.2103.06431` — context on game-testing practice.
