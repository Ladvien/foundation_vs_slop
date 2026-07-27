# BACKLOG.md — Foundation vs. Slop

The containment loop, organized into **area pushes**: vertical slices each cohesive enough for one person or pair to drive to "done," bundling the core capability with the UI, relationships, determinism, and content that support it. Item IDs (`FVS-<epic>-<n>`) are stable across reorganizations — nothing was renumbered or dropped; the horizontal epics (A–N) were re-clustered into the nine pushes below.

---

## 1. The vision

Pivot from *win-by-wiping-the-level* to **win-by-containing** — the "C" in the Foundation's Secure/Contain/Protect. Capture (not kill) an anomaly, **research** it to reduce uncertainty about its hidden parameters, and spend that knowledge on **unlocks** that make the next, harder capture tractable. Killing stays possible but yields nothing; the safe-kill-vs-valuable-capture tension is the core loop. Three nested cycles:

- **Encounter / Contain** (seconds–minutes): drive an anomaly's drives/fields into a *containable basin* and hold it while a timer completes. Not HP depletion.
- **Expedition / Secure + extract** (10–30 min = one procedural seed = one "Branch universe"): locate, contain, and **extract** a target. Win = extraction; lose = squad wipe *or* breach *or* site overrun.
- **Site / Protect + research** (persistent, roguelite): captured anomalies live at **Site-67**, a hand-authored hub built around the **ASYNC door** — a stable anomalous aperture onto the Backrooms, which is why the Foundation put a Site there and why an MTF stages from it. Research unlocks capabilities *derived from contained anomalies* (SCP **Thaumiel** logic — use the contained to contain). The unlock graph is a player-facing difficulty **curriculum**. Doors to further places make the "each seed is a Branch universe" framing spatial rather than abstract.
- **Operative / know + misremember** (persistent, per-person): operatives accumulate **beliefs** about kinds of anomaly — firsthand, witnessed, told, or read — which propagate by conversation and by filed reports, and which change how they behave. Knowledge deliberately **cuts both ways**: it raises fear when the subject is present, and it is the only thing that makes a containment rule legible. This is the progression system; there are no levels. Full design: `docs/2026-07-26-site-hub-and-operative-knowledge.md`.

**The antagonist is canon (from `lib.rs`).** "Slop" is the deliberately ugly, uncanny-valley monster churned out by **SCP-9191, a rogue monster-generating AI** — literal *AI slop*. All antagonist/endgame/research theming derives from SCP-9191, and the endgame theme is **restoring curation/quality against an out-of-control generator**. The earlier "semiotic decay / SCP-2521 / Gat-Hayes stabilizer" reading is **deprecated** and appears nowhere in this backlog except as optional flavor that must not contradict SCP-9191.

**This reunifies the game with `docs/lore/`.** Capture is a *team* verb → it forces the 4-role party (combat/researcher/psionic/xenobiologist); the Thaumiel tree instantiates the tech curriculum; each seed-as-Branch gives the multiverse framing a mechanical home. The shipped game's divergence from the lore docs closes through the loop, not through more lore.

---

## 2. Engine baseline (Bevy 0.19, shipped 2026-06-19)

Assumed by every item; stated once here rather than repeated. Verified against the 0.18→0.19 migration guide; re-confirm starred spellings on docs.rs at implementation.

- **Containment state = added/removed components**, not an enum field: `Uncontained` / `BeingContained` / `Contained` / `Killed`. Only `Contained` carries a rewarding `on_add` **component hook** — so *killing yields nothing* is enforced by the type system, not by branching.
- **Component hooks** (`on_add`/`on_insert`/`on_remove`/**`on_discard`** — note `Replace`→`Discard` rename) are the stable path for invariants; **observers** (spelled **`On<Add, C>`**, *not* `Trigger<OnAdd, C>` ★) for softer fan-out logic.
- **Built-in entity relationships** (`#[relationship]` / `#[relationship_target]`, mature since 0.16, richer accessors + nested query access in 0.19) for parasite↔host, squad↔member, device↔anomaly, Site↔specimen.
- **Session flow = `States` + `SubStates`.** State-scoped entities are always-on; `DespawnOnExit`/`DespawnOnEnter` fire on same-state transitions — use **`NextState::set_if_neq()`** ★ to avoid.
- **Determinism levers:** `Schedule::set_executor(SingleThreadedExecutor::new())` for the FixedUpdate sim; keep the sim in `FixedUpdate`/`Time<Fixed>`; never `par_iter` the core; `sort_total!` any tie-prone iteration (archetype order is not a stable total order).
- **Resources are now components** (`Resource: Component`): don't co-derive both on one type; add `Without<IsResource>` to broad queries; generic `ResMut<T>` needs `T: Resource<Mutability = Mutable>`.
- **Save/load = `bevy_world_serialization`** (renamed from `bevy_scene`: `Scene`→`WorldAsset`, `SceneRoot`→`WorldAssetRoot`). BSN/`bsn!` is code-driven only (no `.bsn` loader yet) — keep procedural spawning in code.
- Also available: `#[require(...)]` components, fallible systems (`-> Result` + `?`), `Command` associated `Out` type, `SystemParam::get_param -> Result`.

---

## 3. Phased roadmap

| Milestone | Delivers | Pushes | Gate to advance |
|---|---|---|---|
| **M0** | A resolvable session (placeholder win) — *not yet capture* | **P1** | State-machine golden test green on x86 (and ARM once P8/J-3 lands) |
| **M1** | The capture verb — three containment archetypes | **P2** | One target can be driven to `Contained`; a kill demonstrably yields nothing (asserted) |
| **M2** | Real captures across the existing roster | **P3** | SCP-610 capturable via quarantine; ≥3 anomalies routed to a stub Site |
| **M3** | The meta loop — research → unlock → persistence | **P4 + P5** | A full capture→research→unlock→harder-capture path playable across two persisted expeditions |
| **M4** | Adaptive difficulty + the SCP-9191 endgame | **P6 + P7** | Retrained archive loads at MODE_COUNT 29; I-1 ablation shows capture-favoring seeds are selectable; endgame trigger fires |
| **M3+** | Site-67, the ASYNC door, and operative knowledge | **P5 + P10** | An operative who has *met* an anomaly behaves differently from one who has only heard of it; a specimen is visibly held in a cell |
| **Continuous** | Determinism/CI + engine/housekeeping | **P8, P9** | Front-load J-1/J-2 (they live in P1/P2); harness lane gating once archives stabilize |

> **Milestone status, audited 2026-07-27.** M0 (P1) and M1 (P2) are **met**. M2 (P3) is met **except
> SCP-610**, which is deferred on the missing asset. **M3 is NOT met**, and it is worth being precise
> about why, because a great deal of M3 did ship: Site-67, the ASYNC door, specimens visibly held in
> cells, save/load, and the whole research/economy/belief *logic* are all in and green. What is missing
> is the **wiring** — a player can capture and extract and come home and look at a specimen, but cannot
> research it, cannot spend a budget, and carries no beliefs. See FVS-E-5, FVS-P-3, FVS-O-1b. M4 (P6/P7)
> is untouched and correctly sequenced behind I-1 → H-1.

**Dependency spine:** P1 → P2 → P3; P2 → P4 (research needs captured specimens); P4 ↔ P5 (unlock hooks ↔ tech-tree; persistence needs posteriors); P5 → P6 (director needs a retrained archive + resolved fitness); P6 → P7 (endgame needs the generator/difficulty spine). P8 is continuous with two items pulled early into P1/P2. P9 is continuous and independent.
**Wiring caveat added 2026-07-27:** the spine assumes an item's *logic* landing means the push advanced. Three items broke that assumption (E-5, P-3, O-1b). Read a "LANDED" marker as a claim about code, and the "Done when" as the claim about the game — they were not the same thing in Push 4, Push 5's economy, or Push 10.
**P10 (operative knowledge) is largely independent** — O-1/O-2 need only the existing drives and containment HUD, so it can run in parallel with P3/P4. Its later items converge: O-3 wants K-3's dialogue content, O-4 wants the Site (G-4) and save/load (G-2), and **O-5 is the payoff that ties the whole antagonist together** with K-4. **N-10 (asset conversion) does NOT block the Site — corrected 2026-07-26.** It blocks the *shipped-quality* Ozea art pass only. `assets/kenney_prototype-kit/Models/GLB format/` already holds **145 `.glb`** (walls, corners, four doorway variants, sliding doors, floors, columns, stairs, crates, indicators, floor buttons, signage numerals), licensed and in-repo. A **greybox** Site-67 is buildable today with zero conversion — and greyboxing first is correct for a hand-authored hub anyway: prove the layout and the loop, then spend the art budget on the meshes the Site actually uses.

---

## 4. The area pushes

**Site-67 and operative knowledge have a full design document** — `docs/2026-07-26-site-hub-and-operative-knowledge.md`. Push 5 and Push 10 items reference its sections; read it before starting either.

Each push lists a **goal**, the **vision tier** it serves, its **reading list** (keys resolve in §6; `[STIG]` etc.), and its items. Per item: a one-line description, **Done when** (acceptance), **Deps**, **Size** (S/M/L/XL), **Touches** (real modules), a **Determinism** flag, and **Reading**. `— (no corpus resource)` means an engine/design task with no honest corpus grounding; it is not an omission.

---

### Push 1 — Session Loop & Win/Lose  ·  Tier 2  ·  M0
**Goal:** make a session *resolve* — terminal states, per-run teardown, and a placeholder win — so the state machine is proven before any capture mechanic exists.
**Reading:** [TEST-OW], [ABM], [ECS]
**Done when:** a headless golden test drives a fixed seed to both Victory (placeholder timer) and Defeat (wipe) with exact-hash reproducibility, and a run can be *re-entered* — `QUIT TO TITLE` → `NEW RUN` yields a genuinely fresh world (A-5). The persistent-Site exemption is **not** in this push: it needs G-1, so A-4's remainder moves to P5.

> **Design correction (2026-07-25) — the win/lose decision cannot live in `AppState`.** `AppState` is registered only by `UiPlugin` in `lib::run`; `tests/replay.rs::ui_never_leaks_into_deterministic_core` *asserts it is absent* headless (`src/ui/state.rs:10-14`), so A-3's golden test as originally written was unimplementable. Resolved by splitting **decision** from **presentation**: a new harness-visible `src/session/` module owns `RunState` (`States`), a latched `RunOutcome` resource, `RunClock`, and the single `FixedUpdate` writer `resolve_run`; `AppState` keeps only the screens and mirrors the outcome. This is also the only shape under which A-5's run-scoped world construction can run headless.

- **FVS-A-1 — Terminal states: sim-side `session` module + screens** · M · ✅ **LANDED 2026-07-25**
  New `src/session/` (`RunState{Active,Resolved}`, `RunOutcome{Undecided,Victory,Defeat(cause)}`, `RunClock`, `resolve_run`), registered in **both** `lib::run` and `sim_harness` like every other gameplay plugin. `AppState` gains `Debrief`/`GameOver`/`Victory` as a pure screen layer, driven by a windowed-only mirror using `NextState::set_if_neq`.
  *Shipped:* `src/session/mod.rs` (+`SessionPlugin` in both `lib::run` and `sim_harness`), `src/ui/debrief.rs` (Victory/GameOver/Debrief screens + the one-way `mirror_run_outcome`), three new `AppState` variants, F10 dev force-victory (debug-only, sends a *message* so `resolve_run` stays the single writer, and a real defeat still beats it).
  Two design points worth keeping: the **latch is `run_if(resource_equals(RunOutcome::Undecided))`**, not `RunState::Resolved` — `NextState` applies in `StateTransition`, which runs *before* `RunFixedMainLoop`, so a frame catching up several sub-steps would re-resolve before the state landed. And the wipe is read from **`Health`, not entity existence**: keying on "any `Unit` entities left" would need an ordering edge after `squad::despawn_dead_units`, and that edge makes Bevy insert an `ApplyDeferred` that flushes despawns *earlier in the tick than they flush today* — a gameplay change smuggled in by a scheduling constraint. Reading health needs no edge and the existing schedule is untouched.
  *Verified:* golden hash unmoved; `ui_never_leaks_into_deterministic_core` still green. · *Deps:* — · *Reading:* [ECS]
- **FVS-A-2 — `RunPhase` SubState** · S · ✅ **LANDED 2026-07-25**
  `RunPhase` SubState (`source = RunState::Active`, **not** `AppState::InGame`): `Locating`/`Containing`/`Extracting`. Sourcing it sim-side is what lets P2's containment systems read it headless.
  *Shipped:* `session::RunPhase` (`#[source(RunState = RunState::Active)]`). Scaffolding only — nothing drives it until B-3. · *Deps:* A-1 · *Reading:* [ECS]
- **FVS-A-3 — Placeholder "survive N ticks" win + golden test** · M · ✅ **LANDED 2026-07-25**
  Swappable `WinCondition` (enum; P2 adds the `ExtractContained` variant). There is **no wave system** in the repo — the original "reads existing time/wave systems" was wrong; the clock is a `RunClock` tick counter incremented once per `FixedUpdate`, not wall time. Authored in a new `session:` slice of `config.ron`; deliberately **excluded** from `WorldConfig` (evolving the win condition would change what "win" means between rollouts and make archive fitness incomparable) — the reasoned exception to the "every feature must evolve" rule, recorded in code.
  *Shipped:* `WinCondition::SurviveTicks` from a new `session:` config slice; `tests/session.rs` (6 tests) plus harness readers `run_outcome`/`run_ticks`/`kill_squad`/`autogib_ready`/`step_until_autogib_ready`. Both terminal paths assert the outcome **and** same-seed hash equality; the wipe localizer runs 10 reps **under CPU load** (invariant 8).
  *Two harness defects found and fixed on the way* — both now TESTING.md invariants, because they mis-lead any future timing test, not just this one:
  1. **`step(n)` advances the fixed schedule `n-1` times on a fresh `App`.** Bevy's `Time<Real>::update_with_instant` returns without advancing on its first call, so the harness's first `update()` runs no fixed tick. Structural, not a race. `step` was deliberately **not** "fixed": every committed golden is defined in terms of `step(n)`, so a literal `n` would move all of them for nothing. Pinned by `a_fresh_app_runs_one_fewer_fixed_tick_than_harness_steps`.
  2. **Killing units early races the async fracture bake.** `autogib::bake_autogib` self-gates on GLB streaming and its doc asserts "combat can't start before scenes load, so the bake is a completed prerequisite of any death" — true in play, false in a test that kills at t=1s. Measured under load on one seed: **45 gib chunks vs 160**, with `gib_hash` splitting one tick after the kill while actors and fields still agreed — precisely the silent cascade `gib_hash`'s docs predict. Waiting for the bake is *not* sufficient either: the wait is a variable number of ticks, so gating on it and killing immediately compares two different sims (that mistake turned 2 distinct results into 5). The fix is wait-then-advance-to-a-fixed-absolute-tick (`app_at_stable_kill_point`). **No production defect** — but see the new N-7. · *Deps:* A-1 · *Reading:* [TEST-OW] (Observation 4, unstable oracles), [ABM]
- **FVS-A-5 — Run-scoped world lifecycle (NEW, 2026-07-25)** · L · ✅ **LANDED 2026-07-25**
  **The live bug it closed:** `Dungeon::generate` ran at *plugin build* and every creature spawned on `Startup`, so the world was a process-lifetime fact — `QUIT TO TITLE` → `NEW RUN` resumed the **same used map**. "NEW RUN" was a lie.
  *Shipped:* `RunState { Idle, Active }` + a four-phase `RunBuild::{World, Grids, Populate, PostPopulate}` chain on `OnEnter(Active)`; a `RunSeed` that starts at the configured seed (so the first run — and every golden — is unchanged) and splitmix64-advances on `OnExit(Active)`; `session::run_scoped()` at all ten spawn roots; the five build-time `Dungeon` readers (`dungeon`, `fog`, `light`, `mold`, `almond_water`, plus `ai::init_fields`) converted to per-run systems. Title `NEW RUN`, pause `QUIT TO TITLE` and debrief `RETURN TO SITE` now drive `RunState`. Pinned by `session::leaving_and_re_entering_a_run_builds_a_fresh_different_world`.
  **Two design corrections the implementation forced — keep these, they are not obvious:**
  * **`RunState` has no `Resolved` variant.** The settled design had one; it broke immediately. Resolving a run transitioned `Active → Resolved`, which fired `OnExit(Active)` — despawning the **entire world at the moment of victory** (the debrief would render over nothing) and resetting the outcome that had just been written. `Active` therefore means *"a world exists"*, not *"the run is unresolved"*; the outcome lives in `RunOutcome`, and **leaving** the run is what tears the world down. `resolve_run` now writes only the resource, never a state.
  * **The camera is split, not run-scoped.** It must outlive a run (the title screen needs one), so `setup_camera` stays on `Startup` and reads no `Dungeon`; a new `focus_camera_on_spawn` re-aims it per run. A `DespawnOnExit` on the camera would have blanked the title.
  *Also:* `RunState::Idle` is load-bearing — Bevy runs `StateTransition` **before** `PreStartup`, so a default of `Active` would build the world before a single asset existed. `PostStartup` leaves `Idle`, and the frame's own transition builds before the first fixed tick. · *Deps:* A-1 · *Reading:* [ECS], [ABM]
- **FVS-A-4 — DespawnOnExit run-teardown hygiene** · S · *determinism: teardown not mid-tick* · **SPLIT 2026-07-25**
  The teardown half is subsumed by A-5 (you cannot verify "run entities despawn" without a re-entry path). What remains here is only the **Site exemption**, which genuinely needs G-1: exempt the persistent Site from run teardown; `set_if_neq` to avoid same-state despawns.
  *Done when:* leaving a run despawns run entities but not the Site; no leaked observers. · *Deps:* A-5, G-1 · *Touches:* `src/session/`, Site module · *Reading:* [ECS]
- **FVS-D-2 — Squad↔member relationship** · S · ✅ **LANDED 2026-07-25**
  *Shipped:* a bodiless `Squad` roster node (no `Transform`/`Health`, so it is invisible to `snapshot_hash` and to the liveness actor count) owning every operative through `MemberOf` / `SquadRoster` — the repo's **first** use of Bevy's built-in relationships. `spawn_unit` gained a `squad: Entity` parameter so *every* unit carries `MemberOf` and the hashed squad stays in one archetype; the Research Room's dev spawner threads the same node.
  **Gotcha pinned by `tests/squad.rs`:** Bevy expresses an empty relationship target by **removing** the component, not by leaving an empty one. A bare `Query<&SquadRoster>` therefore matches *nothing* on a wiped squad, which reads as "no squad" rather than "no survivors" — read it as `Option<&SquadRoster>`. P2's role differentiation will hit this first.
  Despawn hygiene needs no system: the relationship's own hooks drop a despawned member, so there is never a stale `Entity` to dereference after a death. · *Deps:* — · *Reading:* [ECS]
- **FVS-J-1 — Single-threaded sim executor** · S · *determinism: core enabler* · ✅ **SATISFIED 2026-07-25 — no code change**
  The guarantee this item asks for already exists, by a *different* mechanism than the one written here: `sim_harness::build_headless_app_unfinished` pins the global `ComputeTaskPool` **and** rayon to one thread before any plugin initializes, and **asserts it won the init race** (both are process-global `OnceLock`s). `Schedule::set_executor(SingleThreadedExecutor::new())` was never added and **must not be** — it is a second mechanism for one invariant, and two paths to the same guarantee is exactly what makes a determinism regression untraceable.
  *Evidence:* `deterministic_core_is_bit_identical`, `deterministic_core_is_bit_identical_across_many_builds`, `ui_never_leaks_into_deterministic_core` and — per invariant 8, the only probe that counts — `search_rollouts_are_reproducible_under_load` all green on x86.
  *Decided:* the **windowed** build stays multi-threaded. The harness is the determinism oracle; a windowed session is not a golden and never will be, so pinning it would cost frame time to guarantee something nothing reads.
  *Landed in:* `TESTING.md` invariant 5 (rewritten with the reasoning + the [ABM] parallel-across-replicates grounding). · *Touches:* docs only · *Reading:* [ABM] §2.34, [TEST-OW]

---

### Push 2 — Containment Core  ·  Tier 1  ·  M1
**Goal:** the capture verb — a data-authored containment-rule model, the FixedUpdate system that runs it, the outcome hooks (kill-yields-nothing), and all **three archetypes** scaffolded. Swap A-3's placeholder for "extract one contained anomaly."
**Reading:** **[STIG]** (backbone — cite at header), [STIG-AD], [PHERO-V], [SDT-00], [ABM], [TEST-OW], [ECS]
**Done when:** an anomaly can be driven to `Contained` via a rule read against the fields; a kill produces zero specimen/research (asserted by test); the containment HUD reads why it's progressing/breaking.
**Note:** the three archetypes are genuinely distinct — do not pitch "one thrown sphere." Single-target *captures a body*; area-denial *bounds a region*; source-elimination *caps a structure* (which is honestly kill-for-no-specimen, not capture).

- **FVS-B-1 — `ContainmentRule` data model** · M · ✅ **LANDED 2026-07-25**
  *Shipped:* `src/containment/rule.rs` — `ContainmentRule { requires: Vec<FieldCondition>, hold_secs, break_on_fail }`, `FieldCondition { channel, sign, threshold }`, `Sign::{AtLeast, AtMost}`, `OnBreak::{Reset, Keep}`. Pure data + a pure predicate: no ECS, no `App`, 8 unit tests in the hard gate, and **no golden impact** (nothing registered on `FixedUpdate` yet).
  *Grounding:* [STIG] §1 names this exact mechanism — *"the qualitative effect … may of course be internally controlled by some **threshold mechanism acting on a quantitatively varying input**"* — which is why the predicate reads the shared stigmergy channels rather than anything private to the anomaly. Every existing depositor (gunfire, gaze, dread, noise) is therefore already a containment *tool*, and a new channel becomes one for free.
  *Design decisions worth keeping:*
  * **`sign` is load-bearing, not sugar.** `ai::field` already documents `ATTENTION` as read "with **opposite signs**" by different creatures, so 1048 is contained by keeping attention *high* (out-watch, C-3) while the mould pole is contained by keeping it *low*. The polarity lives in the data so there is one evaluator, not two.
  * **Conjunction only — no OR.** An "either route" rule reads to the player as two different procedures and makes the L-1 HUD's "why is this progressing?" ambiguous. Two routes = two rules and an explicit choice, never a hidden branch.
  * **Fails closed.** An out-of-range channel evaluates to *unsatisfied*, and `validate()` rejects an empty rule, a non-positive `hold_secs`, a non-finite threshold and a dead duplicate clause — a bad rule is a content bug that must fail at the door, not an anomaly that captures itself. NaN satisfies neither sign.
  * **`unmet()` exists for L-1.** The HUD's acceptance is "players can read *why* containment is progressing/breaking"; a bare bool cannot answer that, so the predicate reports which clauses fail. · *Deps:* — · *Reading:* **[STIG]** §1
- **FVS-B-2 / B-3 / B-4 — state, the tick, and the kill-yields-nothing hook** · ✅ **LANDED 2026-07-25** (shipped together — one coherent chunk)
  *Shipped:* `src/containment/state.rs` — `Containment` (phase + accumulated hold + rule), `tick_containment` on `FixedUpdate`, the `Contained` marker with an `on_add` hook, and `Specimen`. 6 unit tests + 4 live harness tests (`tests/containment.rs`), including `the_containment_tick_is_bit_reproducible`.
  **Deviation from the engine baseline, deliberate — read before "fixing" it.** §2 asks for four *marker* components (`Uncontained`/`BeingContained`/`Contained`/`Killed`) with one present at a time. Shipped literally, that toggles markers on hashed entities every time containment starts or breaks, and this codebase has a standing rule against exactly that — `scp1048`: *"Every component is inserted at spawn and never toggled — a flipped marker would split the hashed archetype and make ECS iteration order run-dependent."* `parasite`'s `Infestation`/`MancaMood` are the established idiom. So: **the phase is a value field** on a component present from spawn (archetype never moves), and **only the one-way terminal transition earns a marker** (`Contained`, inserted once, never removed) — which is precisely the case markers are good for, and is what carries the reward hook.
  **There is no `Killed` component, on purpose.** The baseline wanted one with no reward hook. Having *no component at all* is stronger: the only path to a `Specimen` is inserting `Contained`, which only `tick_containment` does, only on completion. A `Killed` marker would still be a *place* someone could later attach a reward. `killing_an_anomaly_mid_containment_yields_nothing` kills a target with a capture genuinely under way and asserts zero specimens.
  *Other decisions:* `tick_containment` is ordered `.after(AiSet::FieldUpdate)` so a rule reads the field this tick's deposits and evaporation already settled (the same "read settled state" edge `unit_movement` uses against `AiSet::Think`) — without it the rule evaluates against a half-updated grid whose contents depend on schedule accident. Each anomaly's update is a pure function of its own transform, its own rule and the shared field — no shared counter, no pick, no RNG — so iteration order cannot change the outcome and no canonical sort is needed. `Specimen` is deliberately **not** `run_scoped()`: it must outlive the expedition (that is the roguelite boundary, G-3), and it carries no `Transform`/`Health` so it stays out of `snapshot_hash`.
  *Deps:* B-1 · *Reading:* **[STIG]**, [ABM], [ECS], **[SDT-00]**
- **FVS-B-5 — Archetype 1: single-target capture device** · M · ✅ **LANDED 2026-07-25**
  *Shipped:* `src/containment/device.rs` — `ContainmentDevice { target, reach }`, `deploy_devices`, `release_finished_devices`. A connecting throw begins the capture and links both ways; a miss (dead target, out of reach, already contained) spends the device and does nothing. 5 unit tests.
  **The device names its target rather than searching for one.** Landing it and grabbing "the nearest eligible anomaly" would be a *pick from a query* — which here means a mandatory total sort plus a stable per-anomaly key that does not exist yet (and `util::nearest_planar_keyed` is explicit that a raw `Entity` id is not one). Naming the target removes the pick entirely: nothing to order, nothing to get wrong. It is also the better mechanic — the player chooses which anomaly to spend a device on.
- **FVS-D-3 — Device↔anomaly relationship** · S · ✅ **LANDED 2026-07-25**
  *Shipped:* `Holding` / `HeldBy`, the repo's second use of Bevy relationships. "Breaking containment clears the link" comes for free in the case that matters — when the anomaly despawns, Bevy's own hooks drop the link, so a device can never point at a dead target — and `release_finished_devices` covers the completed/cancelled cases. Same `Option<&HeldBy>` gotcha as `SquadRoster` (the target component is *removed* when empty), documented on the type. · *Deps:* B-5
- **FVS-B-6 — Archetype 2: area-denial quarantine** · L · ✅ **LANDED 2026-07-25**
  *Shipped:* `src/containment/area.rs` — `Quarantine { radius }` + a `Quarantinable` species marker (inserted at spawn, never toggled, so no archetype churn). The region opens the attempt for anything inside it and a breach closes it; the **rule is still evaluated by the one `tick_containment` path**, so quarantine and device capture share a single evaluator rather than forking.
  *Design point:* **a breach is a cancel, not a lapse.** `OnBreak::Keep` banks progress across a condition lapsing while the anomaly is still inside; leaving the region is a different event — the attempt is over — so the hold is discarded regardless of policy. Pinned by `a_breach_ends_the_attempt_and_discards_the_hold_even_under_keep`. Overlapping regions are `any()`, so stepping between two is not a breach. · *Deps:* B-1, B-3
- **FVS-B-7 — Archetype 3: source-elimination (nest capping)** · M · ✅ **LANDED 2026-07-25**
  *Shipped:* `Capped` (terminal marker, **no hook**) + `SiteSecured { capped, total }`, and the mechanic itself is a **query filter**: `crab::nest_reproduce` now runs `Without<Capped>`, so a sealed nest is not a nest that breeds zero crabs — it is a nest the breeding pass cannot see. `SiteSecured` is *derived* every tick rather than incremented, because a nest can also be destroyed outright and a derived count cannot drift out of step with the world.
  **It grants nothing, and that is the point.** `capping_a_nest_halts_its_breeding_and_grants_no_specimen` fills every hoard past the breeding threshold, runs 120 ticks, and asserts both that no crabs are born and that the specimen count is unchanged. This is the archetype that is honestly "kill the source for no specimen"; giving it a reward would quietly undo the pivot the backlog is built on. · *Deps:* B-2
- **FVS-L-1 — Containment HUD** · M · ✅ **LANDED 2026-07-25**
  *Shipped:* `src/ui/containment_hud.rs` — progress, the hold timer, and **one line per rule clause** marked met/unmet with the current field reading. 3 unit tests over the pure line-formatting function.
  **The acceptance was "players can read *why* it is progressing/breaking", so each line is an instruction, not a status:** an unmet `AtLeast` clause reads `[! ] RAISE OBSERVATION >= 0.50 (now 0.10)`, an unmet `AtMost` reads `LOWER GUNFIRE`. That is the payoff for keeping polarity in the data (`Sign`) and for the rule being a conjunction with no OR — an "either route" rule would leave this readout unable to say which route the player is on. The boundary is inclusive on both signs, matching `FieldCondition::is_met`, so a player sitting exactly on the threshold never sees "not met" while the capture ticks. A test asserts every shipped channel has a player-facing name, so a new channel cannot reach the HUD as "UNKNOWN". · *Deps:* B-3 · *Reading:* [STIG]
- **FVS-B-8 — `ExtractContained`, the run phase, and the PLAYER'S VERBS (NEW, 2026-07-26)** · L · ✅ **LANDED 2026-07-26**
  **The gap this closed, and it was larger than the backlog admitted.** Push 2's goal says "swap A-3's
  placeholder for *extract one contained anomaly*" — that never landed: `WinCondition` still had exactly
  one variant, `SurviveTicks(18000)`, so the game was won by surviving five minutes rather than by
  containing anything. And **all three archetypes had no player input path whatsoever**: nothing in
  `src/` ever spawned a `ContainmentDevice`, spawned a `Quarantine`, or inserted `Capped`. They existed
  only in tests. The one live capture (C-2's SCP-999) was satisfied *implicitly* by squad behaviour,
  since there was no hold-fire verb either. M1 had shipped a substrate, not a verb.
  *Shipped:* `WinCondition::ExtractContained { count }` (contain N **and** return the surviving squad to
  the insertion cell), `session::RunFacts`, `phase_for` finally driving `RunPhase`,
  `containment::extraction` (the zone, at `Dungeon::spawn`), `containment::verbs`
  (`ArmedTool`/`DeviceSupply`/`QuarantineSupply`/`TargetId`/`pick_target`), four input systems in
  `selection`, `laser::WeaponsTight`, `ui::verb_bar`, `sim::ContainmentTuning`, 8 harness readers/drivers,
  and 7 new harness tests + 12 new unit tests.
  **Decisions worth keeping:**
  * **The extraction point is `Dungeon::spawn`** — you leave the way you came in. No new worldgen, no new
    placement rule, legible without a marker, and it is exactly where FVS-G-5's ASYNC door will stand, so
    the door becomes this zone *with a body* rather than a replacement for it.
  * **The win counts live `Contained` anomalies, NOT `Specimen`.** `Specimen` is deliberately not
    `run_scoped()` (it is the roguelite boundary), so counting it would hand expedition 2 a free victory
    on expedition 1's captures.
  * **The phase is DERIVED, not ratcheted.** A capture destroyed before extraction walks the phase back
    on its own; there is no un-advance path to get wrong. Recorded in `RunPhase`'s doc: **nothing in the
    pinned core may `run_if(in_state(RunPhase::..))`**, because `NextState` applies in `StateTransition`
    (before `RunFixedMainLoop`) and a catch-up frame observes only the last write — gating pinned
    gameplay on it would make the simulation depend on frame pacing.
  * **Hold fire gates the BOLT, not the system.** `fire_laser` also refreshes `AimTarget`, which drives
    facing, so a `run_if` would freeze the squad's gaze. Checked rather than assumed that this does not
    endanger C-2's `ATTENTION` clause: `deposit_attention` reads `fog::FogGrid`, which is radius-based
    with **no facing cone**, so holding fire starves `THREAT_GUN` while observation keeps accruing.
  * **`WeaponsTight` is a resource, not a per-unit component** — Push 8 measured resources hash-neutral
    while `MemberOf` (an archetype change) moved the goldens, and `spawn_unit`'s bundle is already at
    Bevy's 15-element tuple cap.
  * **`TargetId` is its own component, not a field on `Containment`** — nests are targetable but carry no
    `Containment`. No uniform aim key existed (999 has `Scp999Seed`, 1048-A/B have `CyanideSmell::id`,
    nests had nothing), and `nearest_planar_keyed` needs one that is never derived from the tied quantity.
  * **Logistics live in `sim::ContainmentTuning` on `SimTuning`, not the `containment:` slice.** That
    slice holds the RULES (what capturing *means*, so not evolvable); these are difficulty, which is what
    the world genome is for. Living on `SimTuning` collapsed the genome wiring from four sites to two —
    `WorldEliteDoc` already carries `sim` and `apply_dim` already assigns it. `world_genome::N` 130 → 136.
  * **`to_u32_count` is deliberately not `to_usize`** (which floors at 1): a device supply of **zero** is a
    real, harder world, so flooring would make the authored `0.0` bound unreachable.
  **Keybindings are constrained, not chosen.** `Digit0`–`Digit9` are the time-control rungs, `Q`/`E`/`WASD`
  the camera, `Escape` the pause menu, and `H`/`T`/`P`/`Space`/`F3`/`F4`/`F6`/`F10` are taken. The verbs
  are **C** device · **Z** quarantine · **X** cap · **F** hold fire; right-click or re-press disarms.
  **A trap that cost three confidently-wrong test failures:** `spawn_squad` clusters the operatives around
  `Dungeon::spawn`, and the extraction zone sits on that same cell — so **a fresh run begins already
  extracted**, and a test that captures before moving wins instantly while proving nothing about the
  walk-out. `tests/session.rs::walk_squad_off_the_pad` exists for exactly that, and the lesson is FVS-M-4's
  verbatim: a reproduction that has not been sanity-checked against its own geometry is not evidence.
  **The goldens did NOT move** — measured, not assumed. Adding `advance_run_phase` to `FixedUpdate` was
  expected to permute the linearisation (Push 8's standing caveat), and it did not, because the new node
  is `.chain()`ed into the existing `.after(HealthDamage)` group rather than left floating. The extraction
  zone carries a `Transform` but **no `Health`**, so it contributes no `snapshot_hash` row, and it spawns
  on `OnEnter` rather than `FixedUpdate`. · *Deps:* B-1..B-7 · *Reading:* **[SDT-13]** (meaningful choice
  vs controlling reward contingencies — why a verb bar and not a multiplier), **[STIG]**, Heylighen 2016
  (negative-feedback quantitative stigmergy — why an `AtMost` clause is a verb)
- **FVS-J-2 — Sort audit / determinism lint for new systems** · M · ✅ **AUDITED 2026-07-25 — no sorts needed**
  Every system added by M0 and Push 2 was audited against the three contracts. **None required a sort**, and that is a design result rather than an omission: each is a *per-entity* update that is a pure function of that entity's own components plus shared read-only state — no pick, no shared counter, no budget, no clamped accumulate, no RNG draw. `session::resolve_run` (an `any()` over a predicate), `containment::tick_containment` (each anomaly reads the field at its own cell and writes only itself), `device::deploy_devices` (each device resolves only its named target), `area::tick_quarantine` (an `any()` over regions), `area::track_secured_sites` (a count). The reasoning is recorded at each site, per the "every sort declares its contract" rule.
  `tests/determinism_lint.rs` passes. **Known limitation, stated rather than papered over:** the lint scans for *sorts*, so it cannot catch an order-dependent system that has none — a `.iter().next()` pick, or a shared counter advanced in query order. It would not have caught the very bug FVS-N-8 records. Extending it to flag those shapes is worth doing and is not in this item's scope. · *Deps:* B-3 · *Reading:* **[TEST-OW]**, [ABM], [TEST-NT]

---

### Push 3 — The Anomaly Roster  ·  Tier 1  ·  M2
**Goal:** wire the roster that already has containment-shaped identity — **SCP-610** (zero code; needs an asset export first, see the C-1 correction), then 999/1048/150/crabs — each with its content and the swarm-behavior decisions. The roster leads with bespoke anomalies; **SCP-173/096 are deliberately deferred to Push 7** (the lore doc lists "leading with SCP-173" as an amateur tell, and they need new engineering, not this substrate).
**Reading:** [STIG], [STIG-AD], [UV-REV], [UV-FMRI], [ECS], [GOAP]
**Done when:** SCP-610 is capturable via quarantine and reads as a "slop" instance; 999/1048/150 capturable via their rules; crab infestation clearable via nest-capping; the two crab-behavior forks are decided and documented.

- **FVS-C-1 — Wire SCP-610 from zero** · L (+ an asset export first) · *determinism: core*
  ⚠️ **Corrected 2026-07-25 — 610 is NOT asset-complete.** The claim "asset-complete + rigged … the most shovel-ready item in the repo" was wrong. There is **no `.glb` anywhere** — not in `assets/`, not in `/mnt/codex_fs/game_assets/SCP_Characters/gltf/`. What exists is a Blender *generator* (`SCP_Characters/src/scp_characters/monsters/scp610.py`, `examples/build_scp610_infected.py`, `tests/test_scp610.py`) plus one reference photo. So C-1 carries a **prerequisite**: run the builder and export `scp610.glb` against the `docs/artist_guide.md` contract. Until then 610 is *less* shovel-ready than 999/1048/150, which have shipped rigs — consider leading Push 3 with those.
  Then: faction membership, field deposits/reads, drives, and a `ContainmentRule` consumed by the quarantine archetype (B-6).
  *Done when:* `scp610.glb` exists and loads; 610 spawns, participates in the shared substrate, is containable via quarantine; killable but yields nothing. · *Deps:* B-6, **610 asset export** · *Touches:* new `src/scp610/`, `src/ai/`, `src/ai/field.rs`, `assets/scp610/` · *Reading:* [STIG], [STIG-AD]
- **FVS-C-2 — SCP-999 befriend-capture** · M · ✅ **LANDED 2026-07-26 — the first real capture in the game**
  *Shipped:* a new top-level `containment:` config slice authoring per-anomaly rules, `ContainmentRules` resource, and `Containment` attached at `spawn_scp999_at` (the shared builder, so a Research-Room F6 blob is byte-identical to a seeded one). Pinned end-to-end by `containment::scp999_is_captured_by_befriending_it_not_by_fighting`, which drives the **shipped** rule from `config.ron` — so the slice parsing, validating and being reachable is part of the test.
  **The rule, and why it is the right tutorial:** `THREAT_GUN AtMost 0.05` **and** `ATTENTION AtLeast 0.25`, held 4 s, `OnBreak::Keep`. Both clauses are satisfied by choosing *not* to fight — holster, and stay with it. That states the whole win-by-containing pivot in one creature, and it reads through the L-1 HUD as `LOWER GUNFIRE` / `RAISE OBSERVATION`. `Keep` (cumulative) rather than `Reset`: a nervous trigger finger should cost progress, not the run, on the first capture anyone performs.
  **The blob moves, and the rule samples its *current* cell** — 999 oozes toward the most-anxious squad member, so befriending it means keeping attention on it *as it moves*. The test tracks it for exactly that reason; flooding its spawn point drops the clause the moment it sets off. That is a mechanic, not a test artefact.
  *Config placement:* the `containment:` slice sits outside `WorldConfig` like `session:` — a rule that defines what capturing an anomaly *means* must not be an evolved objective. There is also a mechanical blocker worth stating so nobody "fixes" it by accident: `WorldConfig` is `Copy` and a rule owns a `Vec<FieldCondition>`. Evolving rule *thresholds* as a difficulty axis is defensible later, but it needs that constraint addressed, not a silent `Clone`. · *Deps:* B-5 · *Reading:* [STIG], [GOAP]
- **FVS-C-3 — SCP-1048 out-watch capture** · M · ✅ **LANDED 2026-07-26 — the second real capture**
  *Shipped:* a `scp1048:` rule in the `containment:` slice, `Containment` + `TargetId` attached at
  `spawn_scp1048_at` (the shared builder, so an F6 bear is byte-identical to a seeded one), and the
  mechanic itself in `replicate::scp1048_scavenge`: a bear standing in ambient `ATTENTION` at or above
  the threshold **cannot scavenge**.
  **The exact inverse of SCP-999, and that is the design.** 999 is contained by *not* looking down the
  sights; 1048 by refusing to look away. `THREAT_GUN AtMost` versus `ATTENTION AtLeast` — one rule
  model, two opposite verbs, which is the payoff for `Sign` being data rather than two evaluators.
  **The AMBIENT field, deliberately — not `enemy::ObservedBySquad`.** That per-entity boolean exists
  (M-1 built it for C-6) and using it here would have been easier. But a boolean is a flag you set,
  whereas the decaying, diffusing field is a place you **maintain** — and that difference is the whole
  creature. It is also why the *same* channel gates the build and completes the capture: suppressing
  the copies and containing the bear are one action, not two things the player must learn separately.
  **One knob, two consumers.** `sim.containment.out_watch_threshold` (0.45) feeds both the scavenge gate
  and the authored rule, so they cannot drift apart. Evolvable, bounded `(0.15, 0.85)`: floored above
  ambient field noise (a squad merely present must not passively suppress the build, or the mechanic is
  free) and capped below saturation (an unreachable bar makes the bear uncontainable *and* the copies
  unstoppable — degenerate either way). `world_genome::N` 136 → 137.
  *Harder than 999 on purpose,* being the second capture anyone performs: 0.45 vs 0.25, twice the hold,
  and `OnBreak::Reset` rather than `Keep` — look away and you start over.
  *Pinned by:* `containment::watching_scp1048_suppresses_its_building_and_looking_away_resumes_it`,
  which asserts **both** directions and bails out loudly rather than passing vacuously if the bear never
  entered `Build` (FVS-N-9's lesson); and
  `the_1048_rule_reads_the_ambient_field_not_a_per_entity_watch_flag`, which pins the ambient-vs-boolean
  distinction as data so a later "simplification" onto `ObservedBySquad` fails the suite.
  *Golden:* **unmoved**, measured. The 1048 spawn gained components and `scp1048_scavenge` gained an
  access set, either of which could have permuted the schedule; neither did. · *Deps:* B-5 · *Reading:* **[STIG]**
- **FVS-C-4 — SCP-150 parasite cure/extract** · M · ✅ **LANDED 2026-07-26 — the third capture, and the only one that is an act of care**
  *Shipped:* `parasite::cure_infested_hosts` + a `CureRequest` marker, and a `scp150:` rule in the
  `containment:` slice. Curing a host clears its `Infestation` and hands the **parasite entity** to the
  one-way `Contained` door, which is what grants the specimen.
  **Why this grants a specimen where nest-capping does not** — the distinction is the pivot in
  miniature. Capping a nest (B-7) destroys a structure and yields nothing: it is honestly
  kill-the-source. Curing a host *recovers the anomaly intact*, and that is only possible because D-1
  keeps the parasite alive and linked at embed instead of despawning it.
  *Ordering that is a design decision, not tidiness:* `cure_infested_hosts` runs **before**
  `gestation_tick`, so a cure requested on the tick a burst would land still saves the host. Treating
  someone at the last second working is the whole tension of the verb.
  *A `CureRequest` component rather than direct mutation*, so there is one writer — the same discipline
  `session::ForceVictory` uses for the dev hotkey.
  *Pinned by three tests, because the interesting failures are the negative ones:* curing extracts a
  specimen and marks **the parasite itself** (not a proxy) `Contained`; an untreated host stays infested
  and yields nothing; and curing a *clean* host is a no-op — the failure mode a cure verb invites is
  spamming it on healthy operatives to mint specimens. · *Deps:* B-5, D-1 · *Reading:* [STIG-AD], [ECS]
- **FVS-C-5 — Crab-nest source-elimination integration** · M · ✅ **LANDED 2026-07-26**
  Two of the three acceptance clauses were already satisfied by B-7 (`nest_reproduce` runs
  `Without<Capped>`; `SiteSecured` is derived every tick). What was missing was the **connection** —
  nothing proved capping actually stopped the swarm, and nothing told the player it had.
  *Shipped:* `containment::capping_every_nest_stops_the_swarm_replenishing`, which fills every hoard
  **past the breeding threshold first** (so "no new crabs" cannot pass vacuously), caps them all,
  asserts `SiteSecured::fully_secured()`, and runs 600 ticks with the population non-increasing. Plus a
  `NESTS n/m` readout on the objective line.
  **The HUD line is not decoration — it is the verb's ONLY feedback.** `Capped` grants nothing and is
  invisible by design (B-7: giving source-elimination a reward would undo the pivot). Without a readout
  the player seals a nest and sees literally nothing happen, which reads as a broken button.
  **Deliberately NO starvation model.** "Swarm attrition follows" is satisfied by the swarm being unable
  to replace losses while the squad kills — capping removes replenishment, the squad does the rest.
  Adding a hunger-kill so the number falls on its own would be a balance change the item never asked
  for, and it would land in the pinned core. Recorded because the absence is a decision, not an
  oversight. · *Deps:* B-7 · *Reading:* [STIG-AD], [STIG]
- **FVS-D-1 — Parasite↔host relationship** · M · ✅ **LANDED 2026-07-26**
  *Shipped:* `InfectedBy` / `Hosting` — the repo's **fourth** Bevy relationship, after
  `squad::MemberOf`/`SquadRoster`, `containment::Holding`/`HeldBy` and `site::HeldAt`/`SiteSpecimens`.
  **Why a relationship when `Infestation.active` already existed.** The bool answers "is this host
  infested"; it cannot answer "**which** parasite, and where is it". C-4 needs the second question,
  because extracting a specimen requires the parasite to be a *thing* rather than a flag.
  **The bool stays and is not redundant** — it is a value field on a component present from spawn, so
  hot per-tick systems read it with no archetype churn on the hashed host, whereas the relationship
  inserts and removes a component. State in fields, links in relationships.
  **`manca_embed` no longer despawns the manca**; it strips `Manca`, `MancaMotion`, `Transform` and
  `Visibility` and inserts `InfectedBy(host)`. Dropping `Manca` is what keeps this **census-neutral**
  (an embedded parasite must not count toward `EpisodeOutcome::manca_alive`) and dropping `Transform`
  keeps it out of `snapshot_hash`, which folds `(Transform, Health)` — so the entity contributes exactly
  what it did when it was despawned outright: nothing. **Golden measured unmoved.**
  Despawn hygiene needs no code: Bevy's own relationship hooks drop the link when either end despawns,
  so a cured or dead host can never leave a dangling `Entity`. · *Deps:* — · *Reading:* [ECS], [STIG-AD]
- **FVS-K-1 — SCP-610 content/FX pass** · M · *determinism: render = SSIM*
  Audio/FX/flavor for wired 610, incl. color-language luminosity (color doc). Read it as an SCP-9191 "slop" instance.
  *Done when:* 610 reads as slop; quarantine has readable feedback. · *Deps:* C-1 · *Touches:* `src/scp610/`, FX · *Reading:* [UV-REV], [UV-FMRI], [TEST-NT]
- **FVS-K-6 — SCP-1048 clip driver was invisible to the harness (FIXED 2026-07-25)** · S · ✅ **LANDED**
  `Scp1048Plugin` (harness-visible) spawns bears **with an `anim::BlendSource`**, and `anim::attach_pose_blenders` — also harness-visible — then wired their `AnimationPlayer` with **every slot at zero weight**. But `anim::drive_scp1048_animation`, the only system that ever sets those targets, lived in the windowed-only `Scp1048VisualsPlugin`. So headless, a bear held a permanently undriven blender, and `liveness::every_wired_figurine_keeps_a_well_formed_pose_blend_through_a_live_run` failed the moment the bear's GLB finished streaming — presenting as a flake, since load timing decided whether it fired. Squad, crab and manca all register their clip drivers in their *harness-visible* creature plugins; 1048 was the lone outlier (introduced by `2ab526b`). Fixed by moving the driver into `Scp1048Plugin`; `Scp1048VisualsPlugin` keeps only the fog hiding. · *Touches:* `src/scp1048/mod.rs`, `src/lib.rs`
- **FVS-K-2 — SCP-1048-A triangle fix** · S · *determinism: SSIM*
  1048-A carries 2.6× triangles from 3 dead UV sets — cheapest asset win + a test-suite slowdown. Strip dead UVs.
  *Done when:* triangle count drops ~2.6×; SSIM unchanged; suite faster. · *Deps:* — · *Touches:* 1048 asset · *Reading:* — (no corpus resource)
- **FVS-M-2 / M-3 — DECISION: crab "numbers-kill" and pounce gates** · ✅ **DECIDED 2026-07-25 (user) — claim rejected, docs corrected**
  **Decision: there is no threshold. The code was right; the README was wrong.** `crab_damage_exponent` already makes a pile-on super-linear, so one or two crabs barely register and a swarm shreds — density is the threat without a cliff. A hard "zero damage under N" gate was rejected on feel: a crab that does *literally nothing* reads as broken, not as tactical. M-3 follows M-2 (the user chose "match whatever M-2 decides"), so pounce damage stays on the same curve — one rule for contact and leap, which is the single explainable mechanic the README was reaching for.
  *Landed:* `README.md` §Crabs — both bullets rewritten to describe the shipped curve and to state the *absence* of a threshold as deliberate, so nobody re-adds it from the old text. **Zero gameplay change, zero golden movement** — this was a documentation defect, and the cheapest correct outcome was to fix the document.

---

### Push 4 — Research Economy  ·  Tier 3  ·  M3
**Goal:** turn a captured specimen into knowledge — a belief over hidden parameters, max-information experiment selection, and a reveal paced to *feel* good. Grounded in Bayesian experimental design + the epistemic-action account of curiosity.
**Reading:** **[PROB-ML]**, [BAYESOPT], **[GRIP]**, [LPM], [SDT-00]
**Done when:** a captured anomaly's stat-sheet fog lifts through experiments ordered by expected information gain, front-loading resolvable reveals, and completing a posterior fires exactly one unlock.

- **FVS-E-1 — `ResearchPosterior` component** · M · ✅ **LANDED 2026-07-26**
  *Shipped:* `src/research/posterior.rs` — a Bernoulli belief per [`HiddenParam`] (Lethality, Contagion,
  CaptureBasin, Proliferation) plus a reveal bitset, attached to the `Specimen` **inside the
  `grant_specimen` hook**. Creating it with the specimen means there is never a window in which a
  capture is banked but unresearchable, and no second code path that could initialise one differently.
  **A Bernoulli per parameter, not something richer** — because every parameter here is a question the
  player asks in words, and "68% lethal" is a sentence while a Dirichlet is a diagram. When a parameter
  needs more than two answers the right move is to **split it into more parameters**, which gives the
  player two things to research and the HUD two things to state.
  **Deliberately NOT `knowledge::Belief` (Push 10).** A posterior is institutional and converges on the
  truth; a belief is personal, transmissible and can be *wrong*. Collapsing them would lose O-5 entirely,
  where false hearsay is the antagonist's weapon. Also why this needs no `Option`: a specimen on the slab
  always has a posterior, whereas an operative may never have heard of the thing ([EPISTEMIC]).
  **The numerical trap it is built around:** at reliability 1.0 a single observation drives the belief to
  exactly 0 or 1, and the next contradicting result divides by zero — one anomalous reading would poison
  a record forever. `observe` clamps below 1.0, so a posterior can always be argued with. Pinned by
  `contradicting_evidence_can_still_move_a_confident_belief`.
  Also pinned: a *negative* result resolves as well as a positive one (certainty of absence is still
  certainty — otherwise a fully-researched harmless specimen looks permanently unfinished), and
  `total_entropy` **sums** rather than averages, so adding a parameter makes research take longer instead
  of diluting the average into looking nearly done. · *Deps:* B-4, D-4 · *Reading:* **[PROB-ML]**, [GRIP], [EPISTEMIC]
- **FVS-E-2 — Experiment model + EIG selection** · L · ✅ **LANDED 2026-07-26**
  *Shipped:* `research::Experiment` + `rank_by_information_gain`.
  **The algorithm is the paper's, not an approximation of it.** Tiwari, Radhakrishna, Gulwani & Perelman
  (*Information-theoretic User Interaction*, DOI 10.48550/arXiv.2006.12638) give the chain-rule identity
  `En(Pr(bb | q)) = En(Pr(bb)) − En(Pr(q))`, from which the greedily-best question is `argmax_q En(Pr(q))`
  — *"to greedily seek knowledge, we should ask the question about which we know the least."* So EIG is
  the entropy of the experiment's **answer** distribution, which is both provably equal to the expected
  posterior-entropy reduction and the cheap one to compute — and the legible one, since "how unsure am I
  what this test will say" is something L-2 can put in words.
  *Three decisions the tests pin:* a **resolved** parameter scores exactly 0, not merely low (offering a
  question the player can already see the answer to reads as broken); **reliability scales the gain**,
  because scoring raw answer entropy would make a coin-flip test look maximally informative; and the
  ranking is **total** (ties break on authored index) so the top suggestion cannot flicker under the
  player's cursor between frames. · *Deps:* E-1 · *Reading:* **[PROB-ML]**, [BAYESOPT], + arXiv:2006.12638
- **FVS-E-3 — Reveal pacing (front-load resolvable surprise)** · M · ✅ **LANDED 2026-07-26 — and the premise was wrong twice**
  *Shipped:* `research::pacing` — `reveal_schedule`, `schedule_is_front_loaded`, `felt_value`, and
  `ExperimentFatigue` (the tunable the item asked for).
  **The interesting part is what the tests refuted.** The obvious reading is that greedy information
  gain must already front-load: it always picks the least-predictable question, so surely the first bite
  is the biggest. **It is not**, for two independent reasons, both found by measurement:
  1. **A reveal threshold is not a learning event.** Measuring the schedule with `total_entropy`
     (which filters out revealed parameters) makes the observation that *crosses* `REVEAL_AT` appear to
     be worth that parameter's entire remaining uncertainty. Measured: `[0.28 ×4, 0.72 ×4]` — rising.
     Fixed by adding `belief_entropy` (all parameters, revealed or not) and pacing on that. The two
     metrics answer different questions — *"how much is left to do"* versus *"how much do we know"* —
     and conflating them is a visible bug, so both now exist with the distinction written down.
  2. **Binary entropy is concave.** Moving a belief 0.5 → 0.8 resolves 0.28 bits; 0.8 → 0.94 resolves
     0.40. Uncertainty falls slowly near the middle and fast near the extremes, so equal-strength
     observations resolve an *increasing* amount each time. Still rising: `[0.28 ×4, 0.40 ×4]`.
  So front-loading genuinely has to be authored, exactly as this item said. `ExperimentFatigue` is the
  mechanism and it is diegetic rather than a curve bolted on: **each repeat test on a parameter is
  weaker than the last** — the obvious experiments get run first, then you are down to marginal ones
  arguing over a specimen that has already told you most of what it will.
  **A third bug the floor exists for:** at `decay = 0.55` a 0.8 test drops to 0.44, and **below 0.5 a
  test does not weaken, it LIES** — a Bayesian update at `r < 0.5` moves the belief *away* from the
  observed result. `USELESS_BELOW` stops fatigue decaying through it, and an exhausted test is not
  offered rather than offered-and-inert.
  *Also corrected, in the test:* a leading **plateau** is right, not a failure. Four independent
  parameters equally unknown means the first test on each is worth the same; there is no reason the
  second *question* should reveal less than the first. What must fall is the arc across **rounds**.
  `felt_value` scores bits resolved *this step*, per [GRIP]: the felt quality is in the movement toward
  grip, not in holding it. · *Deps:* E-1, E-2 · *Reading:* **[GRIP]**, [LPM]
- **FVS-E-4 — `Researched` marker + unlock hook** · S · ✅ **LANDED 2026-07-26**
  *Shipped:* `research::unlock` — a `Researched` marker with an `on_add` hook, an `Unlocks(Capability)`
  payout carried per specimen, and `finish_completed_research` sweeping completed posteriors.
  **A hook, not a scanning system**, for the same three reasons `containment::Contained` uses one: it
  fires exactly once at command-apply time; it adds **no schedule node**, so it cannot permute the
  `FixedUpdate` linearisation and move the goldens; and it makes "one completion, one unlock"
  structural rather than asserted.
  **Idempotence is by construction, twice over** — the marker is inserted once and Bevy's `on_add` does
  not re-fire, and the flags are a set. Neither is an `if already_done` guard, and a test pins both so
  removing one does not silently survive on the other.
  A specimen with no authored payout completes and grants nothing, loudly: that is a content gap, not a
  crash. · *Deps:* E-1, F-1 · *Reading:* [SDT-00], [ECS]
- **FVS-F-1 — Tech-tree flags resource + graph** · M · ✅ **FLAGS LANDED 2026-07-26; GRAPH LANDED 2026-07-27 (see F-3)**
  *Shipped:* `research::TechTree`, a bitset resource of `Capability` flags, deliberately **not**
  run-scoped (unlocks are meta-progress and outlive the expedition, exactly like `Specimen`), plus the
  four anomaly-derived capabilities the current roster earns.
  ~~**The prerequisite GRAPH is not built.**~~ **Built 2026-07-27 in `research::curriculum` — see
  FVS-F-3.** The instinct recorded here was right: the graph was authored once the roster had grown into
  a shape worth encoding, and it came out as a three-node chain toward SCP-150 rather than the flat
  four-node list a guess would have produced. One node (`RemoteCapping`) still has no parent, and that
  is a genuine design gap rather than an authoring one — F-3 explains why.
  **FVS-F-2's rule is enforced here rather than remembered.** `every_capability_is_named_as_a_verb_not_a_number`
  walks the table and fails on any label containing `%` or `+`. Every entry reads as something you can
  *do* — "DEPLOY REMOTE OBSERVER", not "+15% observation". If a new capability can only be described as
  a percentage, that test is where it gets caught. · *Deps:* — · *Reading:* [LPM], **[SDT-00]**
- **FVS-L-2 — Research/EIG HUD** · M · ✅ **LANDED 2026-07-26; the dead wiring FIXED 2026-07-27 by FVS-E-5**
  *Shipped:* `src/ui/research_hud.rs` — the stat sheet (one line per hidden parameter, with reveal
  state) and the ranked experiment offers, **each stating the bits it would buy**.
  **Printing the reason is the requirement, not decoration.** L-1 set the pattern for containment: an
  unmet clause reads as an instruction rather than a status, because the acceptance was "players can
  read *why*". A bare ranked list is a black box that happens to be sorted — the player cannot tell a
  good ordering from a broken one. So every offer carries `+0.28 bits`, in **bits** rather than a
  percentage of nothing-in-particular.
  *Three wording decisions the tests pin:*
  * An **unresolved** parameter reads `UNRESOLVED (68%)`, never `68% LETHAL`. The second invites the
    player to act on a guess as though it were a finding, which defeats the point of a fog-of-war sheet.
  * A **resolved** one states a verdict in *both* directions — `RULED OUT` as well as `CONFIRMED` —
    because certainty of absence is a finding, and a specimen proven harmless must not read as blank.
  * The offer list is **bounded to 3**. An unbounded list buries the top suggestion, which is the one
    thing the ranking exists to surface.
  Two empty states are distinguished rather than both rendering as nothing: `RESEARCH COMPLETE` (the arc
  paid out) versus `NO INFORMATIVE TEST REMAINS` (everything resolved, not yet marked complete). An
  empty panel reads as a bug.
  ⚠️ **And an empty panel was what it rendered (found 2026-07-27).** `AuthoredExperiments` was defined in
  this file and **inserted nowhere**, so `update_readout` early-returned on its first line and the panel
  never updated for a whole session. The `readout()` function underneath it was correct and had six
  green tests — which is exactly why it passed review, and is the lesson: *a pure function with full unit
  coverage tells you nothing about whether anything calls it.*
  *Fixed 2026-07-27:* the resource is **deleted** rather than populated, and the panel reads the authored
  `research:` slice keyed on the studied specimen's `subject` — one source of truth instead of two. The
  panel also gained the species name, the `[R] RUN THE TOP TEST` affordance, and the gated state
  (`AWAITING PRIOR RESEARCH: <capability>`), which follows FVS-L-1's rule that a blocked thing must name
  what unblocks it. · *Deps:* E-2, E-3 · *Reading:* [PROB-ML], [GRIP]
- **FVS-E-5 — The RESEARCH VERB: wire Push 4 into the game (NEW, 2026-07-27)** · L · ✅ **LANDED 2026-07-27**
  *Shipped:* `containment::Specimen` gained `subject: knowledge::Subject` (threaded from a new field on
  `Containment`, so every one of the three capture paths must state a species or fail to compile); a new
  top-level **`research:` config slice** (`src/research/curriculum.rs`) authoring the per-species
  experiment battery, ground truth, unlock payout and prerequisite graph; `src/research/lab.rs` — the
  verb itself (`StudySubject`, `RunExperiment`, `run_experiments`, `ExperimentLog`); `R` at the Site;
  `grant_specimen` now attaches `Unlocks` from the curriculum; and the research HUD reads the live
  curriculum instead of the resource nothing inserted. Fatigue state persists. **21 new tests, hard gate
  683 → 704.**
  **Pinned end to end by `studying_a_specimen_to_completion_grants_its_authored_capability`** — the
  acceptance the M3 gate actually asks for, and the test that immediately found FVS-E-6 below.
  *Decisions worth keeping:*
  * **The species lives on `Containment`, not in its own component.** Every path to `Contained`
    constructs a `Containment` first, so making it a constructor argument turns "a specimen whose
    species is unknown" into a **compile error at each spawn site** rather than a runtime surprise in
    the hook. Present from spawn, never mutated — the hashed archetype does not move.
  * **Reuses `knowledge::Subject` rather than minting a species enum.** The kind a specimen *is* and the
    kind an operative holds beliefs *about* must be one key, or FVS-O-4's records office would have to
    translate between two vocabularies that can drift.
  * **The draw is seeded from `(captured_tick, subject, param, prior_runs)`** — all recorded state, no
    accumulator, no entity id. That is FVS-N-8's lesson applied prospectively; it also gives the right
    fiction, since repeating a test you already ran does not resample the universe.
  * **Windowed-only** (`Update` + `AppState::Site`), like `persist`. So the research economy adds **no
    `FixedUpdate` node** and cannot move the goldens — by construction, not by discipline.
  * **The prerequisite gate is on OFFERING research, not on the payout.** Gating the payout would
    dead-end a player who studied out of order: work spent, capability gone.
  *Not done here:* FVS-L-3's Site/tech-tree screen and its specimen selector — `keep_a_study_subject` is
  an explicit placeholder that picks the most-uncertain available specimen by a total key.
  · *Deps:* E-1..E-4, L-2, F-3 · *Reading:* **[PROG]**, [PROB-ML], [GRIP], [SDT-00]
- **FVS-E-6 — The reveal threshold and the fatigue curve are mutually unsatisfiable (FOUND 2026-07-27)** · M · ⚠️ **NEEDS A DESIGN DECISION**
  **Found the first time anything actually ran an experiment**, which is the point: three numbers that
  are each individually reasonable cannot all hold.
  * `ExperimentFatigue::decay = 0.8` with `USELESS_BELOW = 0.5` allows only **three** tests on a
    parameter (`0.8 → 0.64 → 0.512 → 0.41`, and the fourth would *lie*, not merely inform weakly).
  * Three concordant readings at those reliabilities multiply to a likelihood ratio of ≈7.5 — a belief
    of **0.882**.
  * `REVEAL_AT` is **0.9**.
  So a 0.8-reliability battery **could never resolve a single parameter**, no specimen could ever
  complete, and no capability could ever be unlocked — while every unit test stayed green, because each
  piece is correct alone and nothing composed them. Same failure class as the wiring gaps, one layer down.
  *Shipped as a stopgap, not a fix:* `ResearchConfig::check_resolvable` simulates the best case (every
  reading concordant) under the shipped fatigue and **refuses to load** a battery that cannot finish, so
  this can never be invisible again. The authored batteries were retuned into the band that works.
  **The band is the problem, and it is the user's call.** With `decay = 0.8` the usable range is only
  **[0.82, 0.89]**: below 0.82 a parameter never resolves, at 0.90 it resolves in a *single* test. So
  `reliability` — the knob meant to express how hard an anomaly is to study — can currently express
  three outcomes (2 tests, 3 tests, or broken). The authored ramp is a thin 8/8/10 tests across the
  three anomalies. Three ways out, and they trade against each other:
  1. **Raise `decay` toward 0.9** — 5 tests per parameter, a much wider reliability band, longer arcs.
     Costs a re-measure of FVS-E-3's `schedule_is_front_loaded`, whose 0.8 was *measured*, not guessed.
  2. **Lower `REVEAL_AT`** from 0.9 — resolves sooner, but weakens what "the Foundation knows this" means.
  3. **Allow several experiments per parameter** — already supported (fatigue counts per *parameter*),
     so a second, weaker test on the same question extends the arc without touching either constant.
  *Done when:* a decision is recorded and the constants agree; `schedule_is_front_loaded` re-measured
  after any `decay` change. · *Deps:* E-3, E-5 · *Touches:* `src/research/pacing.rs`,
  `src/research/posterior.rs`, `assets/config/config.ron` · *Reading:* [GRIP], [LPM]
  **The gap this closes, and it is the M3 gate itself.** Push 4 shipped `src/research/` as an excellent,
  well-grounded, well-tested **pure library** and never connected it to a player. Four independent
  breaks, each verified by repo-wide search:
  1. `AuthoredExperiments` is never inserted (see L-2 above); `ExperimentFatigue` is never constructed
     at runtime.
  2. **Nothing ever calls `ResearchPosterior::observe`.** Its only non-test caller is `pacing.rs`'s pure
     `reveal_schedule` analysis function. A posterior therefore never moves during play.
  3. **No captured specimen carries an `Unlocks` payout.** `grant_specimen` spawns
     `(Specimen, ResearchPosterior, HeldAt)`; the only non-test `Unlocks` insert is the *save-restore*
     path. So a fresh campaign that completes research hits `unlock.rs`'s documented
     "no payout authored" branch and grants nothing.
  4. **Nothing consumes a `Capability`.** Outside `src/research/`, `TechTree` appears only in
     `persist.rs` and one test. `ArmedTool` still has only the three base verbs.
  So `capture → research → unlock` is reachable today **only through hand-authored test setup**.
  **This is FVS-B-8 happening again one push later** ("M1 had shipped a substrate, not a verb"), and it
  is worth naming as a recurring failure mode rather than a one-off: this repo's house style front-loads
  pure, harness-free logic — which is right — but the green unit tests then read as completeness.
  **The prerequisite nobody noticed:** `Specimen` records **no species**, so nothing can key a payout or
  an experiment battery on *what* was captured. Reuse `knowledge::Subject` (already enumerates exactly
  this roster) rather than minting a second species enum — it then serves the payout table, D-4's cell
  model and O-4's records office at once.
  **Keep the verb windowed-only** (`Update` + `AppState::Site`): research happens between expeditions,
  so it needs no `FixedUpdate` node and therefore cannot move the goldens. The draw against ground truth
  must be **seeded**, not `rand::random`.
  *Done when:* the player can run a ranked experiment on a chosen specimen and watch the posterior move;
  completing a posterior grants the authored capability; that capability is usable as a verb.
  · *Deps:* E-1..E-4, L-2, F-3 · *Touches:* `src/research/`, `src/containment/state.rs`,
  `src/ui/research_hud.rs`, `assets/config/config.ron` · *Reading:* **[PROB-ML]**, [GRIP], **[SDT-00]**

---

### Push 5 — Site, Tech-Tree & Persistence  ·  Tier 3  ·  M3

> **📄 Design settled 2026-07-26: `docs/2026-07-26-site-hub-and-operative-knowledge.md`.** The Director
> chose a **spatial hub** reachable through the **ASYNC door** — a stable anomalous aperture onto the
> Backrooms, which is why the Foundation built a Site around it. Read that document before starting any
> item in this push; three things below change materially:
> * **G-1's Site is hand-authored geometry, not generated.** A hub must be learnable, and `Dungeon` is a
>   single per-run resource — two procedural worlds would be ambiguous. The Site is entities, not a `Dungeon`.
> * **The portal needs no new state machinery.** FVS-A-5's `RunState::Idle ↔ Active` *is* the door; the
>   Site is what exists while `Idle`, which also makes A-4's "exempt the Site from teardown" free (Site
>   entities simply never carry `run_scoped()`).
> * **Squad levelling is rejected — replaced by operative KNOWLEDGE.** Levelling is the archetypal "+X%"
>   that F-2 forbids on self-determination grounds. Instead operatives hold *beliefs* about kinds of
>   thing, acquired firsthand / witnessed / told / read, which propagate through the **existing dialogue
>   system** and across runs through written reports. Knowledge deliberately **cuts both ways**: it raises
>   FEAR when the subject is present, and it is the only thing that makes containment legible. False
>   hearsay propagating is the intended attack surface for SCP-9191 — slop as *misinformation*, with
>   curation as the counter-play, which finally gives `src/dialogue/` and the endgame theme a mechanical job.
> Economy: the **O5 Council grants a budget** rated on the Director's performance, reading the same
> metrics FVS-I-1's fitness must compute. Budget buys **consumables only** — never capabilities, which
> stays on the research side so the soft currency cannot eat the research loop.
**Goal:** the persistent Site, the **Thaumiel** unlock graph (enabling, never numeric), and roguelite save/load. This push and Push 4 together deliver the full three-cycle vision at small scale.
**Reading:** **[SDT-00]**, [SDT-13], [LPM], [QD-PCG], [ECS]
**Done when:** captured specimens persist at the Site across expeditions; each unlock grants a *new verb* (not +X%); a lost run preserves meta-progress; save→reload restores Site/specimens/posteriors/unlocks faithfully.

- **FVS-G-1 — Persistent Site-67 entity** · M · ✅ **LANDED 2026-07-26**
  *Shipped:* `src/site/mod.rs` — a bodiless `Site` root (no `Transform`/`Health`, so hash-neutral), `SiteRoot(Entity)`, and `SitePlugin` on `Startup`, registered in **both** `lib::run` and `sim_harness` because the specimen link is pinned gameplay.
  **The exemption cost nothing, as predicted:** FVS-A-5 made teardown `DespawnOnExit(RunState::Active)` via `session::run_scoped()`, so the Site persists simply by *not* carrying that tag. There is no exempt-list to maintain, and therefore no exempt-list to get wrong. That also closes the remainder of **FVS-A-4**. · *Deps:* A-5 · *Reading:* [ECS]
- **FVS-G-4 — Site-67 geometry + wings** · L · ✅ **LANDED 2026-07-26 — geometry yes, area SEMANTICS no**
  *Shipped:* `src/site/layout.rs` (`AreaId`, the authored `assets/site/site67.ron` — 7 rects, 8 floor runs, 12 walls, 16 props, 6 cells, 1 door, 5 spawns), `src/site/visuals.rs` (floors, walls, props, door frame, cell glazing, 5 Valkyrie avatars), `src/site/nav.rs` (click-to-move over a navigation mask), `src/site/pieces.rs` (the Kenney greybox kit). Hand-authored, not generated, for the reason the design doc gives: a hub returned to every run must be learnable.
  **The validator is the interesting part** — `layout.rs::validate` proves presence of all six required areas, non-overlap, floor coverage, dense cell indices, and **flood-fill reachability** of every area *and* the door from the spawn. It caught its own author during development.
  ⚠️ **`AreaId` has ZERO consumers outside `layout.rs`** (measured 2026-07-27). No entity carries an area tag and nothing triggers on entering a wing, so **only two of the six areas do anything**: AsyncDoor (the trigger volume) and Containment (the cells). Research, Records, Requisition and Briefing are floor rects with a decal beside them. They become real in FVS-E-5 (research), FVS-P-3 (requisition) and FVS-O-4 (records) — the areas are the *placement*, those items are the *verbs*. · *Deps:* G-1 (**not** N-10 — greyboxed from the in-repo Kenney kit exactly as §3 argued) · *Reading:* [ECS]
- **FVS-G-5 — The ASYNC door** · M · ✅ **LANDED 2026-07-26**
  *Shipped:* the door trigger in `site::visuals::enter_the_door` (`NextState::set(RunState::Active)` — **no new state machinery**, exactly as scoped: FVS-A-5's `Idle ↔ Active` already *was* the door), plus `src/site/aperture.rs` + `assets/shaders/async_aperture.wgsl` — the volume inside the frame that visibly is-not-a-room, with its charge eased per-frame.
  ⚠️ **The aperture's uniform defaults are guesses.** The shader was written without ever being looked at; nobody has judged it on screen. It is the game's signature image, so it wants one deliberate art pass — see §7. · *Deps:* G-4 · *Reading:* [UV-REV]
- **FVS-G-6 — `Res<Dungeon>` audit: make `RunState::Idle` a state the game can SIT IN (NEW, 2026-07-26)** · M · *determinism: touches the pinned core*
  **The blocker between the Site existing and the Site being where the game opens.** Today `Idle` is a
  one-frame blip at boot: `session::begin_first_run` leaves it on `PostStartup`, so nothing ever observes
  a world-less frame. Site-67 requires the opposite — the player stands in `Idle` for minutes.
  *Measured 2026-07-26:* flipping `session::AutoStartFirstRun(false)` panics on the first frame —
  `Parameter Res<Dungeon> failed validation: Resource does not exist`, in `selection::command_input`. In
  Bevy 0.19 a missing `Res<T>` **panics**; it does not skip the system.
  **Scope: 90 `Res<Dungeon>` sites across 20 files**, including `fog`, `squad`, `laser`, `enemy`,
  `parasite`, `light`, `mold`, `almond_water` and the whole `ai` tree — many on `FixedUpdate` in the
  pinned core, so this carries **golden risk** and must be measured, not assumed.
  Two shapes to decide between, and it is a real design choice:
  * **Gate on `in_state(RunState::Active)`** — semantically right (none of it means anything without an
    expedition) and one mechanism, but it is a run condition added to pinned systems.
  * **Gate on `resource_exists::<Dungeon>`** — narrower and states the actual precondition, but it is a
    second way of expressing "there is a run", which is the two-mechanisms shape this repo rejects.
  Also worth settling here: the **stale** case. After `RETURN TO SITE` the `Dungeon` resource is not
  removed, it just describes a despawned world — so `resource_exists` is *true* and gating on it would
  not help, while `in_state` would. That asymmetry probably decides the choice.
  **⚠️ RE-SCOPED 2026-07-26 — this is polish, NOT a blocker. Do not do it before the Site ships.**
  Verified: **`Dungeon` is never removed** (`grep remove_resource::<Dungeon>` → 0 hits). It is inserted
  at `RunBuild::World` and persists for the process lifetime, stale-but-present, after every run ends.
  So the world-less window exists **only before the first expedition** — and the fix is a staging
  decision, not a refactor:
  > **Boot → Title → NEW RUN → expedition → debrief → RETURN TO SITE → Site → ASYNC door → next
  > expedition.** The Site is the *between-runs* hub, which is what it is for. `Dungeon` exists from the
  > first `RunBuild::World` onward, so nothing panics, and **zero of the 90 sites need gating**.
  That order is arguably better than opening at the Site anyway: you are dropped in the field, and the
  Site is somewhere you come *back* to — which is what makes the containment wing mean anything, since
  on the first visit it already holds what you just caught.
  What remains for G-6 is only the cosmetic case of making the Site the **boot** destination (a cold
  start with no expedition behind you). Worth doing eventually; worth nothing until the hub renders.
  *Done when:* `AutoStartFirstRun(false)` boots to a stable world-less frame with no panic, the goldens
  are measured (moved or not, deliberately), and the harness path is byte-identical. · *Deps:* — (does
  NOT block the Site) · *Touches:* `src/lib.rs`, ~25 modules, ~50 registration sites · *Reading:* [ECS]
- **FVS-D-4 — Site↔specimen relationship + visible cells** · M · ✅ **LANDED 2026-07-26**
  *Shipped:* `HeldAt` / `SiteSpecimens` — the repo's **third** Bevy relationship — attached inside the `Contained` **hook** (`containment::state::grant_specimen`) rather than by a sweep-up system. Two reasons, both load-bearing: a system would be a new `FixedUpdate` node and would permute the schedule's linearisation for nothing, and the hook keeps *one* path from containment to a banked specimen, so no specimen can come into existence unlinked. A Site legitimately may not exist (bare-`App` unit tests never build one) — that is one optional *link*, not a fallback path; the specimen is granted identically either way.
  *Also shipped:* `specimens_in_capture_order()` and `site::visuals::fill_containment_cells`. **Cell assignment is a pick, and it is keyed correctly:** `SiteSpecimens` is a relationship target ordered by *attach* order, which is not a total order, so the sort is `(captured_tick, captured)` — which is exactly the job `Specimen::captured_tick` was added for. Overflow past the six authored cells `warn!`s rather than silently dropping.
  ⚠️ **Every specimen renders as the same neutral stand-in** (`SitePiece::SpecimenStandin`), because `Specimen` records **no species** — only `captured: Entity` (process-local, deliberately unsaved) and `captured_tick`. That is also the blocker under FVS-E-5's payout table; fixing it once, by giving `Specimen` a `knowledge::Subject`, serves the cell model, the research payout and FVS-O-4's records office together. · *Deps:* G-1, G-4 · *Reading:* [ECS]
- **FVS-F-1 — Tech-tree flags resource + graph** · M · ⚠️ *duplicate entry — the live one is in Push 4 (**PARTIALLY LANDED**: flags yes, graph no). The graph half is FVS-F-3's job; see there.*
- **FVS-F-2 — Enabling (not numeric) unlock effects** · M
  Every unlock grants a **new verb / capability** ("999-derived morale field lets you calm 610"), never "+X%." Hard review rule.
  *Done when:* each unlock adds a capability/tool/containment option; a lint/checklist rejects numeric-only unlocks. · *Deps:* F-1 · *Touches:* tech-tree, equipment, containment · *Reading:* **[SDT-00]**, [SDT-13]
- **FVS-F-3 — Thaumiel dependency mapping to roster** · L · ✅ **LANDED 2026-07-27 (with one open content gap)**
  *Shipped:* the graph and the authored chain, in `src/research/curriculum.rs` + the `research:` config
  slice. **SCP-999 → morale field → SCP-1048 → remote observer → SCP-150 (the goal).** Each edge is
  Thaumiel logic rather than an arbitrary gate: the 999-derived morale field is what makes standing
  still next to a bear while its copies are abroad a thing you send operatives to do; the 1048-derived
  remote observer holds `ATTENTION` on the host's cell without an operative there, which is what frees a
  pair of hands to administer a cure. `Curriculum::progression` derives the order by post-order DFS and
  `the_progression_never_introduces_a_subject_before_its_prerequisite` pins the topological guarantee.
  Validation refuses a cycle, a prerequisite nothing grants (a soft-locked campaign), a duplicated
  subject, and two subjects granting one capability.
  ⚠️ **`RemoteCapping` is granted by nothing and is currently unearnable.** Not an authoring oversight —
  it is FVS-B-7's design landing where it must: capping a nest deliberately yields **no specimen**
  (rewarding source-elimination would undo the win-by-containing pivot), so there is no crab specimen
  for its capability to derive from. Either the crab line needs a capture that is not capping, or
  `RemoteCapping` needs a different parent, or it should be cut. **A design decision, not a config edit.**
  This is also why F-1's acceptance is only *mostly* met: the graph parses, flags persist, and a node is
  gated on its prerequisites — but one node has no path to it at all.
  *Also landed here:* the un-built half of **FVS-F-1**. Its Push 4 entry can be closed.
  *Original scoping note, kept because it decided the shape:*
  **Grounded 2026-07-27 in [PROG], and it changes the authoring direction.** Wang et al. model progression content as a **directed graph whose edges are "harder than"** and derive the presentation order by **post-order DFS from the goal**. Two consequences worth taking:
  * **Author the graph BACKWARDS from the hard capture, not forwards from the easy one.** Their finding is that a good progression is **goal-driven**, not merely gradual: it should build toward a hard task *as fast as the prerequisites allow*, because "engagement often comes from a sense of accomplishment after completing hard tasks." Forwards-authoring produces a smooth ramp with nothing to aim at; backwards-authoring produces the boss level and the shortest honest path to it.
  * **Post-order DFS gives the prerequisite guarantee for free.** The traversal is topologically sorted, so a capability can never be offered before the capture that unlocks it — which is exactly F-1's un-landed "a node unlocks only when prerequisites are met", obtained from the data structure rather than from a check that could be forgotten.
  Their second characteristic, **pacing** (difficulty rising with the player's skill), is deliberately *not* authored here — that is FVS-H-3's job, and duplicating it in static content would put two systems in charge of one thing.
  **Scope note:** with SCP-610 deferred, the roster is 999 / 1048 / 150 / crab nests and four capabilities. That is enough for a real graph (a 4-node DAG with a genuine goal) but not a deep one; the graph must stay data-authored so 610 and C-6's 173/096 extend it without a code change.
  *Done when:* a playable path exists from first easy capture to a hard capture gated only by earlier unlocks; the traversal order is derived, not hand-sequenced. · *Deps:* F-1, C-2..C-5 (~~C-1~~ deferred) · *Touches:* tech-tree, containment, config RON · *Reading:* **[PROG]**, [LPM], [QD-PCG], [SDT-00]
- **FVS-G-2 — Reflection-based save/load** · L · ✅ **LANDED 2026-07-26 — for the subset it covers**
  *Shipped:* `src/persist.rs` — `SaveGame { version, run_seed, tech_tree, specimens }`, atomic tmp+rename write to `$XDG_DATA_HOME/FoundationVsSlop/campaign.ron`, loaded once on `Startup.after(site::spawn_site)` and saved `OnEnter(AppState::Site)` (on arriving home, not on quit, so a crash cannot eat the campaign). Windowed-only: it keys off `AppState::Site`, which the harness never enters.
  **`Specimen::captured` is deliberately NOT serialized.** It is an `Entity` — a process-local arena index — and FVS-N-8's root cause is the standing proof that persisting an allocated id across processes is meaningless. Same reasoning, different subsystem.
  **A version mismatch REFUSES rather than migrating**, and **loading is a replacement, not a merge** (`apply_save` despawns every existing `Specimen` first; merging would silently double a campaign on each load). Both are the right call for a game under construction and both are pinned by tests; revisit the first when the format stabilises.
  ⚠️ **Coverage is about 40% of FVS-G-3's list.** Not saved: operative **beliefs**, the **O5 standing/budget**, the operative roster, filed reports. Each lands with the item that wires it (Phase 3 for beliefs, FVS-P-3 for the budget), and each is a `SAVE_VERSION` bump. · *Deps:* G-1, E-1, F-1 · *Reading:* — (no corpus resource)
- **FVS-G-3 — Roguelite meta boundary** · M · *design doc §6*
  What persists vs resets. **Decided 2026-07-26: operatives PERSIST across runs, carrying their knowledge.** Persists: operatives + their beliefs, specimens, research, unlocks, filed reports, the O5 standing. Resets: the `RunSeed`'s world, run-scoped entities, the run clock/outcome.
  **Consequences of persistence that must be designed for, not inherited:** losing an operative is a permanent loss of everything they knew, so **death must be rare and legible** — routine attrition would reset the meta-loop constantly and knowledge would never compound. Reports become *insurance* (a voluntary hedge against your own death) rather than the only memory. Veterans diverge, which makes squad selection a real decision. **Watch for veteran lock-in** — one operative accumulating everything while the others rot; the natural counter-pressure is already in the design, since fear accumulates alongside knowledge and the veteran is also the most afraid.
  *Done when:* a lost run preserves meta-progress; a won run banks the extracted specimen; a dead operative's unwritten knowledge is gone. · *Deps:* G-1, A-1 (done) · *Touches:* `src/site/`, `src/session/` · *Reading:* [SDT-00]
- **FVS-P-1 — O5 performance review + budget** · M · ⚠️ **LOGIC LANDED 2026-07-26, WIRING ABSENT — see FVS-P-3**
  *Shipped as pure functions* in `src/site/o5.rs` (7 unit tests, **no plugin and no systems**): `ExpeditionReport { squad_size, survivors, captures, extracted, breaches }` — deliberately the same terms `squad_ai::surprise::EpisodeOutcome` carries, cross-referenced at `surprise.rs:621`, so "how did that expedition go" has one definition surfaced twice; `Rating {Exemplary, Satisfactory, Displeased}` + `rate()`, where extraction and ≥1 capture are the hinge; `allowance()`; and `O5Standing::record()`.
  **The budget floor is settled, and it is a price rather than a number:** `BUDGET_FLOOR = Consumable::CaptureDevice.price()`. The floor's job is not generosity — it is that the loop stays *attemptable*. A Director who cannot afford to contain anything is in a state the game offers no way out of.
  **There is deliberately NO "relieved of command" band.** A review that can end a campaign is a second lose condition competing with the squad wipe, and a worse one: it fires from accumulated mediocrity rather than from anything the player can watch happen.
  ⚠️ **None of it runs.** `O5Standing` is never `init_resource`'d and `record()` is never called — nothing builds an `ExpeditionReport` from a finished run. That is FVS-P-3. · *Deps:* G-4 (~~I-1~~ — the shared *terms* were enough; the fitness does not have to exist first) · *Reading:* [SDT-00], [QD-PCG]
- **FVS-P-2 — Requisition: consumables only** · M · ⚠️ **LOGIC LANDED 2026-07-26, WIRING ABSENT — see FVS-P-3**
  *Shipped:* `Consumable {CaptureDevice 30, QuarantineCharge 50, Medkit 20}` and `O5Standing::buy()`.
  **P-2's actual rule is enforced as a test, not remembered:** `nothing_purchasable_is_a_capability` walks `research::Capability::ALL` and fails if the budget can buy any of them. Keeping the two economies disjoint **by kind** is what stops the soft currency from eating the research loop — a checklist would have rotted.
  ⚠️ **`buy()` is never called**, there is no requisition screen, and nothing carries a purchased consumable into an expedition — so the "Done when" is unmet. FVS-P-3. · *Deps:* P-1 · *Reading:* **[SDT-00]**
- **FVS-P-3 — Wire the O5 economy into the game (NEW, 2026-07-27)** · M · ✅ **LANDED 2026-07-27**
  *Shipped:* `src/site/review.rs` + `O5Plugin` — `O5Standing`/`ExpeditionTally`/`Requisitioned`
  registered, the report filed on `OnEnter(AppState::Debrief)`, a requisition panel at the Site
  (`B`/`N`/`M`), and purchases carried into the next expedition. Budget and standing persist.
  **Two decisions worth keeping:**
  * **The report is filed at the DEBRIEF, not on leaving the run.** FVS-A-5 tears the world down on
    exiting `RunState::Active`, which `RETURN TO SITE` does *after* the debrief — so the debrief is the
    last moment living operatives, live `Contained` anomalies and uncapped nests can still be counted.
    Filing on `OnExit` would race those despawns.
  * **Purchases go to a separate `Requisitioned` store, not straight into `DeviceSupply`.**
    `verbs::reset_verbs` zeroes the pouch from tuning at *every* insertion, so a purchase written
    directly into it would be wiped at the start of the very expedition it was bought for. They are
    folded in `.after(reset_verbs)` instead, and a device bought but unused is not silently lost.
  * `squad_size` is **snapshotted at insertion**, because `despawn_dead_units` removes the dead — so
    counting `Unit` at the debrief yields survivors with nothing left to compare them against.
  P-1/P-2 shipped 264 lines of correct, tested economy with **no plugin, no systems, and no resource**. Four separate gaps, measured: `O5Standing` is never registered; `record()` has no caller; `buy()` has no caller and no UI; the budget is not in `SaveGame`.
  **The requisition wing already has a place to stand** — G-4 authored the rect and the decal; what it lacks is the verb (same shape as G-4's `AreaId` note).
  *Done when:* finishing an expedition produces a rating and an allowance the player can see; a purchase increments `DeviceSupply`/`QuarantineSupply` and that supply survives into the **next** expedition; the budget round-trips through save/load. · *Deps:* P-1, P-2, G-2 · *Touches:* `src/site/o5.rs`, new `src/ui/`, `src/persist.rs`, `src/containment/verbs.rs` · *Reading:* **[SDT-00]**
- **FVS-L-3 — Site + tech-tree HUD** · M · *determinism: render*
  Show specimens, the Thaumiel graph, and locked/unlocked state.
  **Also carries the specimen SELECTOR that FVS-L-2 is already waiting on:** `ui/research_hud.rs` picks "the least-researched" specimen as a deterministic placeholder and says in a comment that it does so *"until FVS-L-3's Site screen offers a selector"*. Until then the player cannot choose what to research.
  *Done when:* players navigate the curriculum graph and see prerequisites; a specimen can be selected for study. · *Deps:* F-1, G-1, **F-3** (a graph with no prerequisites has nothing to navigate) · *Touches:* UI, Site, tech-tree · *Reading:* [LPM], [SDT-00]

---

### Push 6 — Adaptive Difficulty: QD Fitness & Live Director  ·  Tier 2  ·  M4 (large, gated)
**Goal:** the differentiator — a runtime archive selector that paces difficulty at the learning-progress band. This is a **new system**, not a rewiring of the static env-var `elite_overlay`, and it is **gated** behind a fitness that actually rewards captures (I-1) and a retrained, non-stale archive (H-1). Cluster I with H: they share the same QD papers, and **I-1 must land before H-3** or the director will surface anti-loop content.
**Reading:** **[QD-PCG]**, [QD], [ME], [QD-OEE], [LPM]
**Done when:** the retrained archive loads at MODE_COUNT 29; I-1's ablation shows capture-favoring seeds are selectable; successive expeditions receive archive-sampled challenges tuned toward intermediate difficulty, reproducibly.

- **FVS-I-1 — Containment/yield fitness terms + kill-vs-capture conflict resolution** · XL · *determinism: offline; mirrors B-4*
  `surprise = W·S·L` has no containment/yield term and actively rewards spectacular **kills**. Add capture-quality/yield terms and resolve the conflict — Constrained Surprise Search (surprise as a *constrained* QD objective) is the direct precedent.
  *Done when:* fitness includes explicit containment/yield terms; a documented decomposition (separate capture-quality archive dimension vs scalarized term) bounds the tension; ablation shows capture-favoring seeds selectable. · *Deps:* B-4 · *Touches:* `src/squad_ai/` surprise/fitness, `coevolve.rs` · *Reading:* **[QD-PCG]**, [QD], [LPM]
- **FVS-I-2 — "Every feature must evolve" coverage lint** · M · *determinism: offline/CI*
  Make CLAUDE.md's rule a lint: flag un-evolved knobs — `GoreSettings.autogib_*` (already caused a 5/5-win→wipe regression), `MetropolisWeights`, most `PerceptionTuning` thresholds, crab/parasite cadence.
  *Done when:* CI enumerates tunable knobs vs genome coverage and fails/warns on gaps; the four families tracked. · *Deps:* — · *Touches:* `src/squad_ai/`, genome defs, CI · *Reading:* [ME], [QD]
- **FVS-I-3 — Wire the RemotePolicy live-trainer hook** · M · *determinism: offline*
  The hook exists but nothing drives it; connect it for live/near-live policy iteration (feeds H-3).
  *Done when:* the hook receives policy updates from a driver; documented advisory until J-5. · *Deps:* H-1 · *Touches:* `src/squad_ai/`, `bin/train.rs` · *Reading:* [ME], [QD]
- **FVS-H-1 — Retrain stale RL policy archive (PREREQUISITE)** · L · *determinism: offline; harness-gated*
  Archives are stale/rejected because MODE_COUNT grew 25→29 (SCP-1048); a multi-hour retrain is required before any live selection is trustworthy.
  ⚠️ **Escalated 2026-07-26 by FVS-B-8.** The synthetic player in `evaluate::run_episode` now performs a
  containment beat, which changes **every rollout trajectory** — so `sweep_prior` must be recomputed and
  every baked world/policy/level archive is stale for a *second*, independent reason. This was accepted
  deliberately (the alternative was a feature the offline search cannot see, which CLAUDE.md forbids and
  TESTING.md invariant 11 explains the cost of). Re-bake before trusting any elite overlay.
  ⚠️ **SEQUENCED 2026-07-27 — I-1 must land BEFORE this, not merely before H-3.** The backlog said "I-1
  must land before H-3". That is too weak: **every** search phase scores through `surprise::fitness`, so
  retraining first bakes an archive optimised against an objective that ignores captures, and I-1 then
  invalidates it. Measured cost of getting this wrong: a full `cargo train all` is realistically a
  **12–20 h** job on this box (from the last real run's logs: `rl` 5 h 38 m across 12 islands, `audio`
  ~4 h, `levels` ~74 s). Doing H-1 first pays that twice. Run `cargo train bench` for this machine's real
  projection before committing the night.
  ⚠️ **The archives are staler than "stale" (audited 2026-07-27).** `elites_squad.ron`,
  `elites_swarm.ron`, `elites_world.ron`, `elites_behavior.ron` and `elites_poet.ron` **do not exist at
  any canonical path** — `evolve3` has never completed, and the 2026-07-19 `behavior` island run was
  killed at gen 10–14 of 30 (1 of 24 islands produced an archive). Exactly **one** bake has ever landed
  (`audio`, 2026-07-19). The policy archives that do exist carry **1225** weights where this build needs
  **1325** (`MODE_COUNT` 25→29), so `NeuralPolicy::from_weights` rejects them loudly — as designed
  ("a stale archive is a re-train, not a resize"). The levels/audio/behavior runs also used held-in world
  `0xB0BA`, **retired 2026-07-19**. So this is closer to a first bake than a re-bake.
  *Also expect:* `baseline_prior.ron` auto-re-sweeps on the first prior-backed search, because
  `ensure_prior_fresh` is mtime-driven and `config.ron` is newer.
  *Done when:* retrained archive loads at current MODE_COUNT; smoke test shows non-degenerate policies. · *Deps:* **I-1** (blocks H-3) · *Touches:* `src/squad_ai/`, `bin/train.rs` · *Reading:* [ME], [QD]
- **FVS-H-2 — Make CMA-MAE emitter reachable (ideally)** · M · *determinism: offline*
  CMA-MAE (`map_elites::map_elites_cma_mae_loop`, Fontaine & Nikolaidis 2023) is implemented and unit-tested but `pub(crate)` and referenced **only by its own two tests** — confirmed dead 2026-07-27.
  ⚠️ **Correction (2026-07-27): the old "otherwise the director is built on the weaker emitter" overstates it.** CMA-**ME** (`map_elites_cma_loop`) *is* reachable via `train rl --cma`, has been used in anger (the 2026-07-23 island run), and is what `train all` already passes for the `rl` phase. So the status quo is CMA-ME, not the isotropic emitter. Also worth knowing before scoping: `rl_search` is the **only** consumer of any CMA emitter — `levels`/`audio`/`behavior`/`evolve3` all use isotropic `map_elites_loop` — so this improves the *policy* archive only unless the wiring is widened.
  Mechanically it means turning `RlSearchConfig`'s boolean `use_cma` into a 3-way emitter enum and threading CMA-MAE's `alpha` (annealing rate) through `SearchArgs`.
  *Done when:* a `train` subcommand runs CMA-MAE end-to-end and writes an archive. · *Deps:* — · *Touches:* `bin/train.rs`, `src/squad_ai/map_elites.rs`, `rl_search.rs` · *Reading:* [ME], [QD], [QD-OEE]
- **FVS-H-3 — `CurriculumDirector` runtime archive-selection system** · XL · *determinism: selection seeded + logged; sampled config feeds the core*
  A **new** resource sampling the QD archive `OnEnter(InGame)` to pick the next challenge — experience-driven/dynamic difficulty. **Not** the static env-var `elite_overlay`. Target the learning-progress band.
  *Done when:* successive expeditions receive archive-sampled challenges tuned toward intermediate difficulty; selection reproducible given seed. · *Deps:* H-1 (ideally H-2), I-1 · *Touches:* new director module, `src/squad_ai/`, `elite_overlay.rs` (read-only ref), app-state · *Reading:* **[QD-PCG]**, [LPM], [QD], [QD-OEE]
- **FVS-L-4 — Curriculum/expedition briefing HUD** · S · *determinism: render*
  Surface the director's chosen challenge as a Branch-universe briefing at run start.
  *Done when:* each expedition shows its sampled challenge framing. · *Deps:* H-3 · *Touches:* UI, director · *Reading:* [LPM], [QD-PCG]

---

### Push 7 — SCP-9191 Antagonist & Late Roster  ·  Tier 3 / endgame  ·  M4–M5
**Goal:** the endgame — SCP-9191 as the generator whose output *is* the uncanny valley — plus the deferred "greatest-hits" roster (173/096) that needs the new per-entity watch primitive. Placed adjacent to Push 6 because **the antagonist is a generator**; its reading list overlaps the QD/generation push, not a narrative silo. The uncanny-valley papers aren't flavor — "perceptual mismatch / atypical features" is *why* generated monsters read as ugly and can drive the generator's output aesthetic.
**Reading:** **[UV-REV]**, **[UV-FMRI]**, [QD-OEE], [QD-PCG], [GOAP]
**Done when:** the endgame trigger fires after a curriculum threshold; confrontation mechanics derive from the SCP-9191 generator theme; 173/096 are capturable via a new per-entity continuous-watch state (explicitly distinct from the ambient field); no shipped copy cites the deprecated semiotic-decay theming as canon.

- **FVS-K-4 — SCP-9191 antagonist reveal + endgame** · XL
  Author the SCP-9191 arc: research cashes out as "restoring curation/quality against an out-of-control generator," culminating in a confrontation with SCP-9191. Deprecate semiotic-decay/2521/Gat-Hayes (optional flavor only if non-contradictory). Consider mining the canon "alarm switch ON/OFF" locker detail for the confrontation trigger.
  *Done when:* endgame trigger fires after a curriculum threshold; confrontation derives from the generator theme; no shipped copy references deprecated theming as canon. · *Deps:* F-3, K-3 · *Touches:* narrative, tech-tree, `lib.rs` lore refs · *Reading:* **[UV-REV]**, **[UV-FMRI]**, [QD-OEE], [QD-PCG]
- **FVS-K-3 — Dialogue content buildout** · L
  `src/dialogue/` (world-space bubbles + RON graph) has exactly **one** authored conversation on a dev hotkey. Author capture/research/Site conversations incl. researcher/xenobiologist role voice.
  *Done when:* ≥N authored conversations trigger from real game events, not a hotkey. · *Deps:* F-3 · *Touches:* `src/dialogue/`, RON · *Reading:* — (no corpus resource; only relevant if you go generative)
- **FVS-C-6 — (LATE) 173/096 + per-entity continuous-watch** · XL · *determinism: FixedUpdate; facing math bit-exact (watch ARM↔x86 f32, J-3)*
  Add 173/096 **only after** the bespoke roster is proven. Each needs a **new** per-entity continuous-observation state (directional/facing check vs a *specific* entity), explicitly distinct from the ambient `ATTENTION` field — new engineering, not a sign-flip reuse.
  *Done when:* a per-entity `ObservedBy`/facing check drives 173/096 freeze/aggro; documented as separate from ATTENTION; capture rules authored on top. · *Deps:* C-1..C-5 shipped, E-*, F-*, M-1 · *Touches:* new watch module, `src/ai/`, `src/enemy.rs` · *Reading:* [GOAP]
- **FVS-M-1 — DECISION: Smiley "observed" definition** · S+M · ✅ **DECIDED + IMPLEMENTED 2026-07-25 (user chose squad LOS)**
  *Shipped:* `enemy::ObservedBySquad` — a **per-entity** component written every `FixedUpdate` by `enemy::update_observation` from `fog::FogGrid`, the same LOS grid that already gates laser targeting and the crabs' `seen_by_squad`. The windowed-only `WatchedByPlayer` resource and `snapshot_player_gaze` are **deleted**; `smiley_reflex` reads the component. Ordered `.after(fog::LosWritten).before(smiley_reflex)` so it reads this tick's visibility. Pinned by `containment::the_watcher_knows_when_the_squad_can_see_it`.
  **Why this mattered beyond tidiness.** The old camera-gaze version put the creature's *defining* mechanic outside the deterministic core: the writer was windowed-only, so headless the watcher read a permanent `false` and no golden could pin the concealment behaviour at all — the single most important thing about this entity was the one thing untestable. It is now core-visible and asserted.
  **This unblocks C-6.** SCP-173/096 each need a per-entity observation state; a global bool was not one. `ObservedBySquad` is that primitive, and it is explicitly *not* the ambient `ATTENTION` field (which is a decaying scalar over cells, read with opposite signs by different creatures) — the distinction C-6 insists on is now real in the code rather than a note.
  *Component discipline:* inserted at spawn and never toggled — only its value flips — so it cannot churn the hashed archetype.
  **The goldens did NOT move — and that is a warning, not a reassurance.** The 1800-tick golden runs with no synthetic player, so the squad idles at spawn and never gets line of sight to the watcher: `looked_at` stays `false` exactly as it did under the old permanent-`false`, and the new path is never exercised. This is TESTING.md invariant 11 verbatim ("coverage of a *system* is not coverage of its *contended* path" — the same blind spot that hid G0 for months). The behaviour is covered by `containment::the_watcher_knows_when_the_squad_can_see_it`, which walks the squad onto the watcher deliberately; **do not** treat the unchanged golden as evidence the mechanic works.
  **Not done:** the Smiley shader/mesh wall-clip bug bundled into this item is untouched; it is a rendering issue with no connection to the observation definition and should be its own item. · *Deps:* — (unblocks C-6) · *Reading:* [GOAP]

---

### Push 8 — Determinism & CI Hardening  ·  cross-cutting  ·  continuous
**Goal:** protect the golden-test discipline as the surface area grows. (J-1 and J-2 already live in P1/P2, pulled early; the remaining items are the CI backbone.)
**Reading:** [TEST-OW], [TEST-NT], [ABM]
**Done when:** the deterministic core runs on ARM and x86 in CI; the harness lane gates merges; new panics/unsafe are blocked.

> **⚠️ `serial_guard` does NOT serialize across test BINARIES (found 2026-07-26).** `cargo test --test a --test b` runs the binaries **in parallel** — `--test-threads=1` only limits threads *within* one binary — and `serial_guard` is a process-local `static`, so two harness `App`s from different targets can and do overlap. That is exactly the interference TESTING.md invariant 4 warns about, and it produced a false `session::both_terminal_paths_are_bit_reproducible` failure that passed cleanly when the target was run alone. Verify a suspicious harness failure by re-running that one target on its own before believing it. A real fix is a cross-process lock (an advisory file lock in `serial_guard`); until then, do not treat a multi-target run as authoritative.


> **Goldens re-pinned twice on 2026-07-25** — once at the end of M0, and again at the end of Push 2 (final values `GOLDEN = 0x3563f0f69281ce4c`, `GOLDEN_FIELD = 0x60b5c51fcc20a281`). The Push 2 re-pin is worth reading: the number landed **back on the value measured before M0's session systems existed**. None of the containment systems writes `Transform` or `Health`, so they can only reach the actor hash by permuting the schedule's topological sort — and adding enough nodes restored the original relative order of the systems that do move actors. Benign, and the cleanest available demonstration of the caveat below.
>
> **Goldens re-pinned 2026-07-25 at the end of M0** (`GOLDEN` `0xc9c8c93f82ab5857` → `0x97abc3221ba2a6f1`; `GOLDEN_FIELD` `0x244e3af59ff9d65a` → `0x2269064bd63a2c44`). Measured once, from a settled tree, with the contributing changes accounted for individually — the full note lives beside the constant in `tests/replay.rs`. Three causes, each isolated by disabling it and re-measuring:
>
> | Change | Moved the goldens? |
> |---|---|
> | Uncommitted in-tree work pre-dating this push (light/gore/health/dungeon/fog/laser + `config.ron`) | **yes** — most of the delta, and not attributable to M0 |
> | FVS-A-1 `session` **resources** | **no** |
> | FVS-A-1 `session` **`FixedUpdate` systems** | **yes** |
> | FVS-D-2 `MemberOf` on every `Unit` (archetype change) | **yes** |
> | FVS-A-5 run-scoped world construction | **no — measured identical before and after** |
>
> Three findings worth carrying forward:
> * **Adding resources is hash-neutral.** Bevy 0.19 makes resources entities, so `SessionPlugin` shifted entity-id allocation for everything spawned after it — and nothing moved. That is real evidence no gameplay path keys off entity ids (the hazard `util::nearest_planar_keyed` exists to prevent).
> * **Adding an unordered `FixedUpdate` system is NOT hash-neutral, and no ordering edge fixes it.** Pinning the session systems `.after(HealthDamage)` gave byte-identical results to leaving them unordered: a new schedule node permutes the linearisation of other unconstrained systems. Budget a re-pin for every future `FixedUpdate` addition (B-3, C-*) and do not read it as a bug.
> * **A-5 moved nothing**, because the first run still generates from the configured seed through the same `GameConfig` seam. That is the strongest available evidence that the run-scoped refactor is behaviour-preserving.
>
> Reproducibility held at every step: `deterministic_core_is_bit_identical`, `..._across_many_builds` and `search_rollouts_are_reproducible_under_load` are all green.

- **FVS-J-3 — macOS/ARM CI lane** · M · *determinism: guards the core*
  Add an ARM lane to catch the known ARM↔x86 f32 divergence (critical once C-6 facing math lands).
  *Done when:* CI runs the core on ARM and x86; divergence is a failure or a documented tolerance. · *Deps:* — · *Touches:* CI config · *Reading:* [ABM], [TEST-OW]
- **FVS-J-4 — clippy denylist vs unwrap/expect/panic/unsafe** · S
  *Done when:* CI fails on new `unwrap`/`expect`/`panic!`/`unsafe` in shipped crates (harness exempt). · *Deps:* — · *Touches:* CI, lints · *Reading:* — (no corpus resource)
- **FVS-J-5 — Make harness CI lane gating** · S
  Promote the advisory (continue-on-error) harness lane to a hard gate once retrain (H-1) stabilizes archives.
  *Done when:* harness lane blocks merge on regression. · *Deps:* H-1 · *Touches:* CI · *Reading:* [TEST-NT], [TEST-OW], [ABM]

---

### Push 9 — Engine & Housekeeping  ·  cross-cutting  ·  continuous
**Goal:** the corpus-free tech-debt and one nav bug, clustered precisely *because* they don't draw on the research spine — safe to hand to whoever has spare cycles.
**Reading:** mostly none; [QD-OEE] for N-2, [ABM] for N-4, [ECS] generally
**Done when:** the large files are split without changing golden hashes, orphaned weight is gone, and the doorway nav bug is fixed.

- **FVS-M-4 — BUG: squad stuck in doorways** · M · ✅ **DOES NOT REPRODUCE 2026-07-26 — regression test added, no code change**
  *Shipped:* `tests/nav.rs::the_whole_squad_traverses_a_one_tile_doorway_without_jamming` — finds a genuine 1-tile doorway (floor, two opposite floor neighbours, walls on the other axis, **open space on both sides**), orders all five units through it, and asserts they all reach the goal. It **passes**: the squad funnels through single-file and gathers on the far side.
  **The symptom appears already fixed by work in the tree**: `WALL_THICKNESS` was widened so a 1-tile doorway now has `TILE − 2·WALL_THICKNESS = 0.72` m clear against a 0.44 m unit (~0.14 m slack per side), up from 0.6 m. `src/squad.rs` still documented the old 0.6 figure — corrected.
  **Two false alarms while building the test, both worth recording because they would fool the next person too:**
  1. The first doorway found in raster order opened into a **one-cell alcove**. Five units cannot fit in one cell, so the squad "failed to traverse" on pure geometry. The finder now requires ≥8 floor cells within radius 2 on *each* side.
  2. The arrival metric read "nearer to `far` than `door` is" — but `far` is the cell immediately past the doorway, so a unit that traversed **and kept walking to the goal** scored as *not arrived*. That reported a jam while the squad was standing on the objective. Now measured against the ordered goal.
  Each of those produced a confident, wrong "3 of 5 units jammed" result. A reproduction that has not been sanity-checked against its own geometry is not evidence.
  **A real defect was found and deliberately NOT shipped.** `FlowField::steer` aims at `cell_center + flow * 1.0` where `flow` is normalised, so a *diagonal* step targets only `0.707` along the diagonal — a point near the shared **corner** of two cells rather than the next waypoint's centre (`1.414` away). Targeting the neighbour cell's centre is the correct steering. But the nav test passes identically with and without that change, so it is **not load-bearing for any demonstrated bug**, and it alters movement inside the pinned core — which would move the goldens and require a balance re-verify for no measured gain. Recorded here for whoever has a symptom it actually explains.
  *Next:* if the README symptom recurs, the untested case is a doorway under **contention** — units moving in *opposite* directions through one opening, which the single-goal test never produces.
- **FVS-N-1 — Split `dungeon.rs`** · L · ✅ **LANDED 2026-07-26 — hashes identical, verified**
  3,447 lines → `src/dungeon/` as `mod.rs` (717), `layout.rs` (657), `rooms.rs` (355), `config.rs` (286), `render.rs` (236), `cutaway.rs` (115), `tests.rs` (1,134). A **pure move**: `use super::*` in each submodule inherits the parent's imports, so the diff is whole items relocated rather than hundreds of rewritten `use` lines, and glob re-exports keep every existing path resolving. `migrated_defaults_reproduce_the_shipped_golden_hash` and `field_passes_are_bit_identical` **passed unchanged** — which is the proof the move changed nothing.
  **Also split into plugins** (the [ECS] plugin pattern: resources are the public API, plugins add plugins). `DungeonPlugin` now owns only the generated grid — the `Dungeon` resource that nav, fog, placement and containment read — and composes two independently replaceable presentations: `DungeonRenderPlugin` (floors/walls/posts) and `CutawayPlugin` (the knee-wall squash). A different art treatment or see-into-rooms trick now swaps one plugin and leaves the simulation untouched.
- **FVS-N-2 — Finish `mycelia/` + `almond_water/` splits** · M · ✅ **LANDED 2026-07-26**
  These were already 11 files; what remained oversized was each file's trailing **test module**. Five extracted (`fruit` 1,891→1,280, `mycelia/mod` 1,661→1,414, `almond_water/mod` 1,142→939, `habitat` 833→507, `perceptual` 862→420) using `#[path]` so the module tree — and therefore `use super::*` inside every test — is completely unchanged.
  *One near-miss worth recording:* `mycelia/mod.rs` had **77 lines of real code after** its test module, and a naive "everything from `#[cfg(test)]` to EOF" extraction silently truncated them. Caught by the compiler, fixed by brace-matching the module instead of assuming it was trailing. Any future test extraction should brace-match, not assume.
  Boundaries preserved: mycelia stays cosmetic-GPU-only (never in `sim_harness`), almond water keeps the shared field kernel in the core.
  **Also split `MyceliaPlugin` into real plugins.** It had become a god-plugin registering nine unrelated concerns, and the tell was already in the source: `fruit::build(app)`, `grazing::build(app)`, `measure::build(app)`, `testbed::build(app)` were **plugin-shaped free functions** — the seams had been found years ago, they just never got the `Plugin` trait, so the boundaries were implied rather than declared. Now `MyceliaPlugin` owns the GPU plumbing and the simulation and **composes** `MoldControlPlugin` (the CPU-authored control texture the compute shader reads — a genuine swap point), `FruitPlugin`, `GrazingPlugin`, `MoldMeasurePlugin` and `MoldTestbedPlugin`.
  **`GrazingPlugin` is the one that matters.** It is the *only* part of this module tree that touches pinned state (crab hunger + the `MEAT` field, on `FixedUpdate`), and it was buried in a nested `build(app)` call. Naming it at the registration site makes the determinism boundary visible where someone is most likely to violate it — the same discipline `Scp999Plugin` / `Scp999VisualsPlugin` already uses.
  *Checked and left alone:* `mold::MoldPlugin` (the CPU reaction-diffusion gameplay mold) is already cohesive — one field, its bake, its update, with the ordering edges its readers depend on. Splitting it would separate systems that share a single resource's read/write cycle, which is the opposite of a boundary.
- **FVS-N-3 — Split `coevolve.rs`** · M · ✅ **LANDED 2026-07-26**
  1,386 lines → `src/squad_ai/coevolve/` as `search.rs` (571), `mod.rs` (293), `tests.rs` (186), `artifacts.rs` (161), `population.rs` (105), `descriptors.rs` (100). One gotcha worth knowing for the next directory-module split: the moved code used `super::surprise` etc., which meant `squad_ai::surprise` while `coevolve` was a *file* module — as a *directory* module `super` now resolves to `coevolve`, so those paths were rewritten to `crate::squad_ai::`. All 10 harness-feature tests still pass.
  **Not a plugin, deliberately:** the offline search owns no systems and no resources — it is called from `bin/train`, never scheduled. Wrapping it in a `Plugin` would add an App-shaped ceremony around code no `App` ever runs.
- **FVS-N-4 — Extract RON-splice/golden machinery from `bin/train.rs`** · L · ✅ **LANDED 2026-07-26**
  2,569 → 1,947 lines; the RON-splice + golden re-pin machinery is now `src/bake.rs` (637) in the **library**, reused by the binary via `use foundation_vs_slop::bake::*`. All 9 of its tests moved with it and now run in the lib rather than a bin target (which is why the lib test count went 592 → 601).
  Worth its own reviewed unit because `train apply` both **changes the sim** and **moves the ruler** that measures it, and `TESTING.md` is explicit that a tool doing both in one step cannot be reviewed. The two safety properties now live in one file: `splice_block` rewrites only the scalars that actually changed (comments, ordering, formatting survive) and `repin_one` refuses to re-pin a golden it cannot uniquely identify.
- **FVS-N-5 — Remove orphaned assets** · S · ✅ **LANDED 2026-07-25**
  Verified unreferenced repo-wide first (nothing in `src/`, `assets/config/`, `tests/`, `docs/` — the only hit was this backlog entry naming them), then removed `assets/hazmat/`, `assets/hazmat_locomotion_pack/`, `assets/kenney_blaster-kit_2.1/`: **10.1 MB**, `assets/` now 52 MB. Hard gate green.
  *Also corrected:* `CREDITS.md` attributed "Blaster kit, prototype kit … `assets/kenney_*`" — the blaster kit is now gone while `kenney_prototype-kit` stays, so the line was narrowed to what actually ships. An attribution table that lists assets the repo no longer contains is worse than no table.
- **FVS-N-10 — FBX/OBJ → GLB asset conversion pipeline (an ART UPGRADE, not a blocker)** · M · *design doc §7*
  ⚠️ **Corrected 2026-07-26 — this does NOT block the Site.** The old claim ("nothing in Push 5's Site work can start without this") was wrong: the repo already ships 145 Kenney prototype `.glb`, enough to greybox all six areas. Do this **after** the greybox layout is proven, so the conversion targets only the meshes the Site actually uses instead of all 411.
  Verified while correcting it: Blender 5.1.2 has `import_scene.fbx` + `export_scene.gltf` enabled under `--factory-startup` (no addon step); the library is **already authored in metres on a 1 m grid** (`SM_Floor_Plain` is exactly 1.0 × 1.0, matching `TILE_SIZE`; `SM_DoorFrame_Double` is 1.98 m, matching `DOORWAY_HEIGHT`), so no unit conversion; **33 of 37 packs carry zero textures** (flat PBR base colours — the artist guide's "strongly preferred" case), only the I series has `*_BaseColor.png` siblings. `assets/low_poly_furniture/` is an undocumented in-repo precedent: it holds both the source `.fbx` and a mirrored converted `glb/` subtree, so the same job was done once before and no script survived — reproduce that output layout. The exporter contract already exists in code at `/mnt/codex_fs/game_assets/projects/scp_characters/src/scp_characters/export/gltf.py` (`export_yup`, `export_tangents`, `export_apply`, `export_image_format`).
  Real counts, measured: **418 `.fbx`** (411 distinct basenames — **7 collide across packs** and would overwrite each other in a flat output dir), 418 `._*` forks (not the ~700 the design doc claims). Two source layouts exist, and `Pack_SciFi_A_001+_V2.0` is nested one level deeper with a `+` in the path.
  *Previously:* The Ozea Studio "Ultimate SciFi Asset Library" (`/mnt/codex_fs/game_assets/models/scifi/ozea_ultimate_library/`, 37 packs, **411 distinct meshes**) covers most of Site-67 — but it ships `.fbx` + `.obj` + `.blend` and **zero `.glb`**. Every one of the 194 `.glb` on the share is a mushroom or an SCP character. `docs/artist_guide.md` §3 is a hard requirement: glTF 2.0 binary only, no FBX.
  Blender 5.1.2 is on this host with a working headless pipeline (see the root `CLAUDE.md`), so this is a scripted batch job, not manual work. Must also honour the artist-guide contract: Y-up, metres, scene 0 is the asset, embedded textures.
  **Strip the macOS resource forks first.** ~700 `._*` files are interleaved through the packs and every `find` for `.fbx` returns them as phantom duplicates — they will corrupt a naive batch importer.
  **Do not bulk-copy into `assets/`.** The share is the *library*; `assets/` holds only what the game loads, converted and named for its use (`docs/artist_guide.md` §2).
  *License:* commercial use and modification permitted; redistributing the pack itself is not — fine for shipping inside a game.
  *Done when:* a repeatable script converts a named pack subset to `.glb` meeting the artist-guide contract, and one converted prop loads in-game. · *Deps:* — · *Touches:* new `scripts/`, `assets/site/` · *Reading:* — (no corpus resource)
- **FVS-N-9 — `a_unit_shooting_on_the_move_keeps_its_legs_running` is intermittent (FOUND 2026-07-25)** · S · *determinism: test-only*
  `#[ignore]`d in `tests/liveness.rs` rather than left red. It failed in 3 of 4 full-suite runs on a loaded box, across code states that both pre-date and post-date FVS-A-5, so it is **not** an A-5 regression.
  What was ruled out by instrumenting it: the figurines *are* streamed and wired (all 5 squad `PoseBlender`s present — a `step_until_squad_blenders_ready` settle was added to `sim_harness` and did not fix it), the squad *is* engaging (`AimTarget` set on 4 units, 43 hostiles alive), and the slot filter *does* match (5 blenders at `LOCO_SLOTS + 2`). So the failure is real: the upper-body action layer takes **no weight at all** over 200 ticks in the failing runs.
  The action layer arms only on the frames a **bolt actually spawns** (`drive_valkyrie_animation` reads newly-added bolts, and `fire_laser` only spawns on the cooldown-wrap tick), so the likely cause is that no bolt is emitted in the window — aiming is not firing. The test's own message ("the squad did not engage") is therefore misleading and should be re-worded once the cause is known.
  **Investigated hard on 2026-07-26; still `#[ignore]`d, but the search space is much smaller now.**
  **The game is not broken — check this first before suspecting gameplay.** `search_calibration::the_authored_brains_produce_a_real_encounter_on_every_world` passes, so squads do engage and fire across every held-in world. The committed replay goldens would *not* have caught a firing regression either, and it is worth knowing why: the 1800-tick golden runs with no synthetic player, so the squad idles at spawn and never fires (TESTING.md invariant 11, the same blind spot that hid G0).
  **Ruled out, each by measurement:** the figurines are streamed and wired (all 5 blenders present at `LOCO_SLOTS + 2`; a `step_until_squad_blenders_ready` settle was added to `sim_harness` and did not help); fog works (91 visible cells); the weight cap allows the assertion (`ACTION_ALPHA = 0.9` vs a `> 0.5` threshold); and the window is not too short (extended 200 → 1200 ticks, still `max_action = 0.000`).
  **The failure is upstream of the animation entirely:** `aimed = 0/5` — with 43 hostiles alive, *no unit ever acquires a target*. Three decoy placements were tried and all read 0: 1.5 m straight ahead (can land inside a wall slab, where fog never marks it visible), on the unit's own cell (degenerate aim direction — `(enemy - unit)` normalised fails the front-arc gate), and a scanned visible floor cell at a 1–3 tile standoff. Since real crabs are also present and also never engaged over 1200 ticks, the decoys are probably not the variable at all — the marching squad simply never closes with anything in this seed.
  *Improvements kept regardless:* the decoy placement now picks a visible floor cell at a real standoff (both prior placements were latent bugs), and the measurement loop breaks as soon as the property is observed instead of always burning a fixed 200 ticks.
  *Done when:* the scenario reliably produces an engagement — most likely by driving the squad *at* a known hostile cluster rather than marching it across the map and hoping — and the test passes 10/10 under load with `#[ignore]` removed. · *Deps:* — · *Touches:* `tests/liveness.rs` · *Reading:* [TEST-NT]
- **FVS-N-8 — Gib spawn positions were order-dependent** · M · ⚠️ **MOSTLY FIXED 2026-07-26 — a RESIDUAL survives under heavy load**
  **`autogib::seed_from` hashed the `AssetId` of the character GLB to seed the fracture.** An `AssetId`
  is a **slot index in the asset arena**, assigned by async load order — so the same mesh got a
  different id run to run, hashed to a different seed, and `fracture` sliced the body along
  **completely different planes**. Measured before the fix: **23 of 23 fragments differing**, in
  `half_extents` as well as `center_local` — the mesh was being *partitioned* differently, not merely
  rounded differently. Every symptom this item ever described followed from that: identical chunk
  counts and keys with positions differing by tens of ULPs, the cascade into
  `crab::assign_meat_targets`, and the load-dependence that made it read as a race.
  Its own doc comment said *"deterministic **within a run**"*, which was true and was the tell — nothing
  compared two runs' bakes until one was written.
  *Shipped:* `seed_from_path`, seeding from the asset **path** (authored, not allocated, identical
  across runs, processes and machines), hashed with a hand-rolled FNV-1a because `DefaultHasher` is not
  guaranteed stable across toolchains and so has no business seeding anything compared between builds.
  A figurine handle with no asset path is **not baked at all** (loud `error!`) rather than silently
  seeded from something unstable — one path, no fallback.
  *Pinned by:* `tests/autogib_determinism.rs` (its own fast target — **1.5 s, no load**, asserting
  centroids *and* half-extents), and `session::gib_spawn_positions_stay_identical_under_load`, which is
  the old `#[ignore]`d reproducer **un-ignored and now green** (five simultaneous deaths under CPU load).
  **Three fixes preceded this one and none of them was the cause. They were all still correct**, and the
  pattern is the lesson worth keeping:
  1. `gore::drain_gore`'s canonical key was a **prefix of its value** (folded `gib.is_some()`, a bool,
     while `GibSource` carries `{source, origin, scale}`).
  2. The autogib bake assembled its **vertex soup in async-load order**, and float addition is not
     associative.
  3. `drain_gore`'s scatter seed was a **`Local<u32>` accumulator** — a death's scatter was a function
     of how many gore events the App had *ever* drained, so one difference anywhere desynchronised
     everything after it, permanently. (This is also why 1 and 2 could not have helped: both corrected
     ordering *within* a tick; that one was an accumulator *across* ticks.)
  **The diagnostic lesson**, paid for over three sessions: *identical counts + identical keys +
  identical ring order + positions differing in the last bits points at the **geometry source**, not at
  the ordering of the code that consumes it.* Two sessions were spent auditing consumers. The question
  that ended it was "is the bake output reproducible?" — a 30-line test with no death in it at all.
  **⚠️ REOPENED the same night — the fix is real but not complete.** With the whole `session` suite
  running **immediately after a full `cargo test`**, `gib_spawn_positions_stay_identical_under_load`
  fails again. Characterised, not guessed:
  * **Reproduces deliberately:** `cargo test >/dev/null; cargo test --features test-harness --test
    session -- --test-threads=1`. Roughly 1 failure per run in that shape.
  * **Does NOT reproduce** running `session` alone and idle (20/20, four consecutive runs), nor running
    *only* that test after the gate (passes). It needs the other 19 tests' load on top of a
    still-settling box.
  * **The bake DOES settle** — the panic is the gib-split assertion at `tests/session.rs:309`, not the
    `step_until_autogib_ready` precondition. So this is not the test giving up early; the gib state
    genuinely still splits.
  So the `AssetId`-seeded fracture was **a** cause and its fix stands (the bake is now provably
  reproducible — `tests/autogib_determinism.rs`, 0 of 23 fragments differing where it was 23 of 23).
  But something downstream of the bake still varies under sufficient load.

  **🔎 Root-cause audit 2026-07-27 — the two suspects above were the wrong two. Read this before
  touching the file.**
  * **`AutogibCache` (suspect 2) is RULED OUT.** All three collections (`body`, `guns`, `baked`) are
    keyed by the figurine scene's `AssetId<WorldAsset>` and **every access in the tree is a point
    lookup** — nothing iterates them, so insertion order reaches nothing. Insertion order does fix the
    order `meshes.add()` allocates fragment mesh handles, but no mesh `AssetId` produced by the bake
    reaches `GibKey`, `Transform`, `Carryable` or `GibRing`.
  * **`GibSeq` (suspect 1) is real, but as an AMPLIFIER, not the origin.** It is App-cumulative with no
    reset anywhere, incremented at two sites inside `drain_gore` (a gib'd `UnitCrunch` is **+2**; a
    gibless crunch or an `EnemySplat` is +1; `FleshHit`/`Viscera` are 0), and folded into `GibKey` by
    both spawners. **That is literally the accumulator shape already fixed for the scatter seed** — the
    fix removed it from the *scatter* and left an identical one feeding the *key*. Since `GibKey` sorts
    `crab::assign_meat_targets` and `carry_gibs` delivery, one event-count difference desynchronises
    everything after it permanently, with no re-convergence. **But the code shows no mechanism that
    makes the count differ**: the kill is pinned to fixed tick 600 and every producer canonically sorts
    before pushing. And if the count *did* differ, the cause would almost certainly have moved
    `snapshot_hash`/`field_hash` too — the test dedups all three, so that would present as actors or
    fields splitting, not gibs alone. **Remove it on its own merits; do not expect it to be the fix.**
  * **⭐ The strongest lead is the bake's OWN input ordering — `autogib.rs`, the vertex-soup sort.** It
    orders `parts` by **`AssetId<Mesh>`**, which is an arena slot allocated by async load order and slot
    recycling — *precisely the value `seed_from` was condemned for using, ninety lines earlier in the
    same file.* The comment beside it asserts "the asset id is stable across same-seed runs
    (measured)", and that is exactly the kind of measurement §1 of the retracted handoff was taken
    under too light a load. The consequence chain is already written down in the file: append order →
    `Soup::centroid()`'s **non-associative float sum** → cut planes shift by ULPs → a vertex crosses a
    plane → the fragment bbox moves → `center_local`/`half_extents` move → the spawn `Transform` and
    `Carryable.weight` move. **That is the recorded N-8 fingerprint exactly** — identical counts,
    identical keys, identical ring order, positions differing in the last bits. `valkyrie.glb` declares
    23 meshes, so this is a ~20-element list whose order that key decides. Note `sort_total!` proves the
    key is *unique*; uniqueness is not **stability**, and a unique key drawn from a load-order-dependent
    allocator still permutes the list.
  * **The cheapest decisive probe:** run `tests/autogib_determinism.rs` **under the same 8-thread load
    `tests/session.rs` uses** (it currently builds two Apps back-to-back in ~1.5 s *idle*, which is why
    it stays green while `session` goes red), dumping the post-sort `parts` sequence per bake. If it
    permutes, the fix is the same move that fixed `seed_from`: key on something **authored** (the mesh's
    asset path/label, or a canonical `Name` DFS path) rather than **allocated**, keeping the matrix bits
    as the instance tiebreak.
  * **Do not aim a fix at the drain seed.** `scatter_seed` and `index_in_tick` **cannot move `gib_hash`
    at all** — they reach only velocity, spin, droplets, spray and pools, and `spawn_meat_chunks`
    discards the seed outright. Physics is off in `deterministic_core`, so nothing moves after spawn.
  * **Same bug class, still latent elsewhere:** `drain_gore`'s canonical key contains `g.source`, an
    `AssetId<WorldAsset>`, justified by "`AssetId` is `Ord`, so the source can go in the key directly."
    Harmless *today* only because all five squad members share one figurine GLB and die at distinct
    positions, so the position components decide first. It becomes live the moment two different
    character sources die simultaneously.
  * **Two more worth closing while in the file:** `bake_autogib` does not gate on *partial* scene
    instantiation (its `all_loaded` check only sees meshes whose entity has already spawned, so a body
    sub-mesh that has not spawned yet is invisible to the gate — and the result is then marked `baked`
    permanently); and `drain_gore`/`confine_gibs`/`cap_gib_chunks` are a bare unchained tuple in
    `Update`, correct only by grace of the one-thread pool.
  * **Generating the load:** background subshells are **not** captured by `kill $(jobs -p)` in a
    non-interactive shell. A previous session left 12 busy-loops at 99% for ~4 minutes and killed an
    unrelated 54-minute replay job. Track the PIDs explicitly and verify they are gone.
  **The claim to distrust is my own:** the commit that fixed the seed said N-8 was closed on the
  strength of a clean 48-minute replay and a green un-ignored reproducer. Both were true, and both were
  measured under a *lighter* load than the one that still breaks it — TESTING.md invariant 13 exactly.
  **Corrects a standing claim in Push 8:** *"adding a resource is hash-neutral ⇒ no gameplay path keys
  off entity ids"* was inferred from the victory/timer path, which never spawns a gib. Registering
  `site::SitePlugin` (one bodiless `Startup` entity) was enough to perturb load timing and turn this
  latent bug into a hard failure of `both_terminal_paths_are_bit_reproducible` — which is how it was
  finally caught. · *Touches:* `src/autogib.rs`, `src/gore.rs` · *Reading:* [TEST-NT], [ABM]
- **FVS-N-11 — One unexplained `tests/session.rs` failure, not reproduced (FOUND 2026-07-26)** · S · *determinism: unknown*
  **Recorded because it happened, not because it is understood.** During a sequential sweep of the fast
  harness targets, `session` reported `16 passed; 1 failed`. The grep in use did not capture which test,
  which is the first lesson: **always capture the failing test NAME**, not just the counts.
  *Not reproduced in four subsequent runs*, including one under **12 busy-loop threads** (114 s versus
  69 s idle, so the load was real and the box was genuinely contended). All 17 passed each time.
  Per TESTING.md invariant 13 — *an exoneration is only as strong as the condition it was measured
  under* — four runs is a weak exoneration for something seen once, so this stays open rather than
  being written off as a flake.
  **The one condition not yet reproduced:** the failing run came immediately after a full rebuild, so
  the test binary started while the box was still settling from a saturating `cargo` build. That is a
  different load *shape* from a steady busy-loop — bursty and I/O-heavy rather than pure CPU — and it is
  the next thing to try.
  Also worth suspecting before anything else: the sweep ran five targets back-to-back in one shell loop.
  That is sequential and should be safe, but invariant 4 exists because "should be safe" about
  concurrent harness `App`s has been wrong before.
  *Done when:* either reproduced and diagnosed, or a documented decision that the sweep shape is the
  cause and the sweep changes. · *Deps:* — · *Touches:* `tests/session.rs` · *Reading:* [TEST-NT]
- **FVS-N-7 — Fracture-bake completion is wall-clock dependent (FOUND 2026-07-25)** · S+M · *determinism: latent*
  `autogib::bake_autogib` self-gates on the figurine's sub-meshes being present in `Assets<Mesh>` — i.e. on async GLB streaming — and documents the premise it leans on: *"combat can't start before scenes load, so the bake is a completed prerequisite of any death."* That holds for a human playing, but it is a **timing assumption, not an invariant**. If a unit dies before its bake lands, the death spawns a completely different gib population (measured under CPU load, same seed: **45 chunks vs 160**), and `gib_hash`'s own docs describe the cascade — a different `Carryable` steers `crab::assign_meat_targets`, so the bisect lands on the crab, not the cause.
  Not currently a shipped bug (nothing kills a unit in the first second of a real run), and it is **not** worth forcing the bake synchronous. What is worth deciding: whether the offline search — which runs thousands of rollouts on evolved worlds where an early death is plausible — should gate its rollouts on `sim_harness::autogib_ready` the way `tests/session.rs` now does.
  *Done when:* documented decision; if "gate it", `squad_ai::evaluate::rollout` waits for the bake before scoring. · *Deps:* — · *Touches:* `src/autogib.rs`, `src/squad_ai/evaluate.rs` · *Reading:* — (no corpus resource)
- **FVS-N-6 — Dedupe shader noise** · S · ✅ **LANDED 2026-07-26**
  `assets/shaders/noise.wgsl` is now the **only** file in the project defining `vnoise`/`fbm` — `almond_water.wgsl` was the last holdout and now `#import`s them.
  **The copies were not identical, and that decided the approach.** Almond water used a different hash (`p3.zyx + 31.32` vs `p3.yzx + 33.33`) and a different lacunarity (2.03 vs 2.0, which is deliberate — the irrational step breaks the axis-aligned banding a clean ×2 leaves, and that is exactly what an organic puddle margin needs). Unifying the *algorithm* would therefore have changed pixels and failed the item's own "SSIM unchanged" bar. So its exact chain moved **verbatim** into the shared library as `hash13`/`vnoise13`/`fbm3_organic`, mirroring the existing `fbm4`/`fbm5` shape where a caller-varying knob is a named entry point rather than a per-shader copy. One home for noise, byte-identical math, no visual change.
- **FVS-J-4 — clippy denylist vs unwrap/expect/panic/unsafe** · S · ✅ **LANDED 2026-07-26 — as a ratchet, not a ban**
  `tests/panic_budget.rs`, in the hard gate. A blanket `#![deny(clippy::unwrap_used)]` is unusable here and that is *why* CI runs clippy `continue-on-error` today: the codebase predates the rule, so a deny fails on line one and gets switched off within a day — and a lint that is switched off is worse than none, because it reads as enforcement while enforcing nothing. This pins the **count** instead: new code cannot add a panic site without handling the error or deliberately raising a committed number, which is a reviewable act. It also fails when the count *drops*, telling you to re-pin downward, so the budget can only shrink.
  **The measurement is the interesting part: the real number is 26, not the ~248 a raw `rg` reports.** Almost all of that grep is test code and prose in doc comments. Test files, inline `#[cfg(test)]` modules (brace-matched — the `mycelia/mod.rs` lesson), `sim_harness.rs` (test infrastructure, and deliberately panicky at determinism preconditions) and `bin/train.rs` (offline tool) are exempt. The shipped simulation carries 26, which is what makes a ratchet realistic rather than aspirational.

---

### Push 10 — Operative Knowledge  ·  Tier 3  ·  M3–M5  ·  **the progression system**
**Design:** `docs/2026-07-26-site-hub-and-operative-knowledge.md` §3 — read it before starting any item here.
**Goal:** operatives accumulate *beliefs* about kinds of thing, beliefs **propagate** between them and across runs, and belief changes behaviour. This **replaces squad levelling**, which would have violated F-2 ("+X%" unlocks) on self-determination grounds.
**Reading:** **[MISPERCEPT]**, [EPISTEMIC], [SDT-00], [GRIP], [ECS]
**Done when:** an operative who has met SCP-1048-A behaves differently near one than an operative who has only *heard* about it; a false belief can spread through the squad and be corrected.

**Why this is not levelling.** A level is a scalar that makes an operative better at everything, everywhere, forever. A belief is a proposition about a **kind of thing** that only acts when that kind is present: contextual, legible ("Okafor knows 1048-A is lethal"), *transmissible*, and capable of being **wrong**. None of that is true of a number going up.

- **FVS-O-1 — `Belief` model + firsthand acquisition** · M · ⚠️ **MODEL LANDED 2026-07-26; ACCEPTANCE UNMET — see FVS-O-1b**
  *Shipped:* `src/knowledge/mod.rs` (354 lines, 8 unit tests, **no plugin and no systems**) — `Subject` (6 append-only variants covering the whole roster), `Claim {Lethal, Harmless, Containable}` with `contradicts()` so `Containable` is orthogonal to the other two, `Provenance {Firsthand 0.85, Witnessed 0.65, Told 0.45, Read 0.35}`, `Belief`, and the `Knowledge` component. `learn()` implements the reliability ordering, so a retelling can never overwrite firsthand experience.
  **Absence of a belief is NOT a low-confidence belief** — the one modelling point not compromised. Fisher, quoted in [EPISTEMIC]: *"not knowing the chance of mutually exclusive events and knowing the chance to be equal are two quite different states of knowledge."* `Knowledge::of` returns `Option`, so "never met it" is a distinct behavioural state from "unsure", pinned by `never_having_met_something_is_not_the_same_as_being_unsure_about_it`.
  **Deliberately distinct from `ai::brain::Fact`** (perception: *is this true right now, sensorily*) and from `research::ResearchPosterior` (institutional, converges on truth). A belief is personal, transmissible, and **can be wrong** — collapsing any two of the three would lose O-5 entirely.
  ⚠️ **`Knowledge` is never inserted on any entity and `learn()` is never called outside tests**, so the item's own acceptance — *"an operative struck by 1048-A acquires a firsthand `Lethal` belief; a bystander does not"* — is **unmet**. That is FVS-O-1b. · *Deps:* — · *Reading:* [EPISTEMIC], [ECS]
- **FVS-O-1b — Attach `Knowledge` to operatives + firsthand acquisition (NEW, 2026-07-27)** · M · ✅ **LANDED 2026-07-27**
  *Shipped:* `Knowledge` inserted at `spawn_unit` (a second `insert`, since the bundle is at Bevy's
  15-element cap — the idiom `host_infestation_bundle` already uses), and firsthand acquisition in
  `scp1048::effects::scp1048_strike_damage`: the operative a copy actually **hits** learns
  `BearCopies is Lethal` at `Firsthand`, and nobody else does. A bystander would acquire a `Witnessed`
  belief, which is FVS-O-3's job, not this system's — so the acceptance's negative half is structural.
  *Determinism:* a value field present from spawn, never a toggled marker, and each strike writes only
  the struck operative's own component — so no canonical sort is needed and the hashed archetype does
  not churn. **Golden impact must be measured** (a new component on every `Unit` is an archetype
  change; FVS-D-2's `MemberOf` moved them). · *Deps:* O-1
  The wiring half of O-1. Insert `Knowledge` at `spawn_unit` as a **value field on a component present from spawn** — never a marker toggled on acquisition, per `scp1048`'s rule that a flipped marker splits the hashed archetype; copy the `containment::Containment` pattern. Note `spawn_unit`'s bundle is already at Bevy's 15-element tuple cap, so it must nest.
  Then the acquisition itself: being struck by 1048-A writes a `Firsthand` `Lethal` belief on **that** operative and on nobody else.
  *Done when:* O-1's stated acceptance actually passes as a harness test. · *Deps:* O-1 · *Touches:* `src/knowledge/`, `src/squad.rs`, `src/scp1048/` · *Reading:* [EPISTEMIC], [ECS]
- **FVS-O-2 — Knowledge changes behaviour, both ways** · M · ⚠️ **WIRED AT GAIN ZERO 2026-07-27 — genuinely inert this time**
  *Shipped:* `src/knowledge/coupling.rs` — `apply_belief_fear` on `FixedUpdate`, ordered
  `.after(AiSet::Drives).before(AiSet::Think)` so it scales the fear the drive rules already settled
  this tick; plus the knob the handoff claimed existed, `sim.belief_fear_gain`, **evolvable** and in
  the world genome (`world_genome::N` 137 → 138, bounds `(0.0, 1.0)`).
  **`0.0` is a bit-exact no-op and the function returns early at it**, so the goldens cannot move for a
  mechanic nobody enabled. Turning it on is a separate, deliberate commit with a measured re-pin — the
  same two-step FVS-B-8 used. Pinned by `a_zero_gain_belief_leaves_fear_bit_identical`, which asserts
  **bit** equality rather than approximate equality, because "nearly no-op" is indistinguishable from a
  regression.
  *Presence, not omnipresence:* a belief only bites within `PRESENCE_RADIUS` of the subject — that is
  the whole distinction from a level, which would apply everywhere forever. The set of present subjects
  is assembled from `Containment::subject` and `Scp1048::variant` rather than a new `AnomalyKind`
  component, so it adds no third source of truth and no component to a hashed archetype.
  *Determinism:* the reduction is a **`max`**, order-independent in `f32` — the same argument
  `drives::track_max_is_independent_of_source_order` pins — so no canonical sort is required.
  **Still to do:** the *benefit* half. `can_read_rule` is still uncalled; gating
  `ui::containment_hud`'s clause lines on it is what makes knowledge legible rather than only costly,
  and it is the half that makes the trade a trade.
  **`HANDOFF.md` said this shipped "wired inert at gain zero". It did not ship at all** (measured 2026-07-27). `Knowledge::fear_scale` and `can_read_rule` exist as pure functions with **no callers anywhere in the repo**, and there is **no gain constant and no config knob** — the "gain zero" staging existed only as a literal `0.0` passed by one unit test. Landing this is a real change (the knob, the `ai/drives.rs` call site, the `containment_hud` gate), not a constant flip.
  **The knob is evolvable and belongs on `sim:`**, not in a rules slice: it is difficulty, which is exactly what the world genome explores (`world_genome::N` 137 → 138). Ship it at gain 0 first and prove the goldens bit-exact, *then* turn it on as a separate reviewed commit with a measured re-pin — the same two-step FVS-B-8 used.
  **Cost:** with the subject in perception, a high-confidence `Lethal` belief raises that operative's FEAR gain — a frightened operative flees sooner, aims worse, breaks a containment hold. **Benefit:** knowledge is what makes containment *legible* — an operative with a `CapturableBy` belief can read that anomaly's rule clauses in the containment HUD (L-1 already renders per-clause state); without it they show as unknown.
  **The asymmetry is the thesis:** understanding a thing is what makes it frightening, and also the only way to contain it. Same trade the research economy encodes (Push 4), pushed down onto the individual.
  *Done when:* a knowing operative measurably fears the subject more, and can read its rule; an ignorant one does neither. · *Deps:* O-1, L-1 (done) · *Touches:* `src/knowledge/`, `src/ai/drives.rs`, `src/ui/containment_hud.rs` · *Reading:* [SDT-00], [GRIP]
- **FVS-O-3 — Propagation by conversation (`Told`)** · M · *determinism: a pick — needs a total sort*
  One operative tells another in the field; confidence decays with each retelling. **Rides the existing dialogue system** (`src/dialogue/`, `squad_ai::dialogue::MemoryStream`, already grounded in Park et al.). That is the point: the dialogue layer currently has **one authored conversation on a dev hotkey** (K-3) and no reason to exist — this gives it one, so authoring conversations becomes gameplay-load-bearing rather than decoration.
  **Determinism:** "A tells B" over a query is a *pick*, so it needs a stable total key — `SquadMember` is the one every other site uses (`tests/determinism_lint.rs`).
  *Done when:* a belief measurably spreads squad-wide through conversation; retold confidence is strictly lower than firsthand. · *Deps:* O-1, K-3 · *Touches:* `src/knowledge/`, `src/dialogue/` · *Reading:* **[MISPERCEPT]**, [ECS]
- **FVS-O-4 — Reports: written and read at the Site (`Read`)** · L
  The cross-run channel. An operative who dies takes their firsthand knowledge with them — but a filed report survives for the next squad. Since operatives **persist** (G-3), a report is *insurance*: a voluntary hedge against your own death, which makes the records office a **choice** ("spend the time writing it up?") rather than mandatory bookkeeping.
  *Done when:* a report written in run N is readable in run N+1 and confers a `Read`-provenance belief; needs save/load. · *Deps:* O-3, G-2, G-4 · *Touches:* `src/knowledge/`, `src/site/` · *Reading:* [MISPERCEPT]
- **FVS-O-5 — False belief as SCP-9191's attack surface** · L · **the payoff**
  Hearsay can be **wrong** and propagates anyway. [MISPERCEPT] supplies the mechanism — pluralistic ignorance, where a false belief survives because everyone assumes everyone else knows better.
  **This is what the antagonist is for.** Slop is not only ugly monsters, it is **plausible garbage** — which is exactly what a false report is. Giving SCP-9191 a way to seed misinformation into the squad's belief network makes the endgame theme ("restoring curation/quality against an out-of-control generator", K-4) a *mechanic the player fights* rather than a narrative frame around unrelated combat. The counter-play is Foundation-shaped: **verify firsthand, and curate the records.**
  *Done when:* a false belief can be seeded, spread, acted on, and corrected by firsthand verification or by purging a report. · *Deps:* O-4, K-4 · *Touches:* `src/knowledge/`, narrative · *Reading:* **[MISPERCEPT]**, [UV-REV]
- **FVS-L-5 — Roster screen: what each operative believes** · M · ✅ **LANDED 2026-07-27**
  *Shipped:* `src/knowledge/roster.rs` — the `L` overlay on the previously-dangling `MenuState::Roster`
  variant, plus `SquadKnowledge`, the cross-run carry that finally implements **FVS-G-3's** "operatives
  persist, carrying their knowledge".
  **Beliefs persist, entities do not.** Operatives are `run_scoped()` and rebuilt every expedition, so
  "persist" has to mean the *beliefs* do: `SquadKnowledge` is keyed by `SquadMember` index, mirrored
  from the living during a run and restored onto the next squad at `RunBuild::PostPopulate`. Making the
  entities immortal instead would fight `RunState`'s teardown, which is the single mechanism that makes
  `NEW RUN` a genuinely fresh world.
  **A dead operative's knowledge is lost structurally, not by a death handler:** the sync **rebuilds**
  the table from the living, so a member with no row contributes nothing and their slot resets. That is
  G-3's stake, and it is what will make FVS-O-4's reports worth writing.
  **Provenance and confidence are both printed**, and that is the requirement rather than decoration —
  FVS-O-5's counter-play is the player noticing a belief is *hearsay* and going to verify it firsthand.
  A line reading only "believes 1048-A is lethal" would hide the one field that makes a false belief
  actionable. An operative who has met nothing reads `NO FIELD EXPERIENCE ON RECORD` rather than blank,
  because "never encountered" is a distinct state from "unsure" (the Fisher point). · *Deps:* O-1
  "Okafor — SCP-1048-A is lethal (firsthand, high confidence)". **The player CAN see beliefs** (Director's decision 2026-07-26, may change). This is what makes a *false* belief spreading something the player can notice and act on, so O-5 depends on it.
  *Done when:* the roster lists each operative's beliefs with provenance and confidence. · *Deps:* O-1 · *Touches:* UI, `src/knowledge/` · *Reading:* [GRIP]

---

---

## 5. Traceability

**Pushes → vision tiers:** Tier 1 (Encounter/Contain) = P2, P3. Tier 2 (Expedition/Secure+extract) = P1, P6. Tier 3 (Site/Protect+research + endgame) = P4, P5, **P10**, P7. Cross-cutting = P8 (determinism/CI), P9 (engine/housekeeping).

**Pushes → corpus (which paper motivates the push):**
- P2, P3 (containment/roster) ← **[STIG]** stigmergy taxonomy (the field-condition/sign-flip model), [STIG-AD] stigmergic behavior design.
- P4 (research) ← **[PROB-ML]**/[BAYESOPT] information-gain acquisition (experiment selection), **[GRIP]** rate-of-uncertainty-reduction (reveal pacing), [LPM] learning progress.
- P5 (tech-tree/persistence) ← **[SDT-00]**/[SDT-13] competence/autonomy + overjustification (enabling-not-numeric unlocks).
- P6 (fitness/director) ← **[QD-PCG]** Constrained Surprise Search (the kill-vs-capture conflict), [ME]/[QD] MAP-Elites/QD, [QD-OEE] open-endedness, [LPM] difficulty band.
- P7 (SCP-9191) ← **[UV-REV]**/[UV-FMRI] uncanny valley (the generator's output aesthetic + why it's unsettling), [QD-OEE]/[QD-PCG] generator framing.
- P10 (operative knowledge) ← **[MISPERCEPT]** pluralistic ignorance (why a false belief survives transmission — the mechanism behind O-5), [EPISTEMIC] ignorance ≠ uniform probability (why "never met it" is not `confidence: 0.5`), [SDT-00] competence/autonomy (why knowledge-as-verb beats levels-as-numbers), [GRIP] the felt value of reducing uncertainty.
- P8 (determinism) ← **[TEST-OW]** open-world-game testing, [TEST-NT] the oracle problem, [ABM] reproducible simulation.

**Honesty ledger.** Nine items carry no corpus resource (K-2, K-3, G-2, J-4, M-4, N-1 [light], N-5, N-6) — engine/design/housekeeping tasks. They are marked, not omitted. `[SDT-00]` is confirmed in-corpus (PDF + 43-page conversion + 104 chunks; its catalog title field is empty, which is why a title check misses it). `[PROB-ML]` and `[BAYESOPT]` are in-corpus with empty title fields — identities inferred from content; verify before formal citation.

---

## 6. Corpus bibliography (reading-list keys)

All present in the local `home-still` corpus (returned with PDF path + chunk index). Flags where catalog metadata is incomplete.

- **[SDT-00]** Ryan & Deci (2000), *Self-Determination Theory…* — 10.1207/s15327965pli1104_01. **Verified** (PDF + 43-pp + 104 chunks; empty title field).
- **[SDT-13]** Vansteenkiste & Ryan (2013) — 10.1037/a0032359.
- **[LPM]** Oudeyer & Kaplan (2007), *What is Intrinsic Motivation? A Typology…* — 10.3389/neuro.12.006.2007.
- **[PROG]** Wang, Cohen, Yi, Park, Teo & Andersen (2019), *Goal-based Progression Synthesis in a Korean Learning Game*, FDG '19 — 10.1145/3337722.3337745. **In corpus** (9 pp, 25 chunks; catalog `title` field empty — added to the housekeeping note below). *Found 2026-07-27 while grounding FVS-F-3; it is the direct precedent for the Thaumiel graph.* Models content as a **directed graph whose edges are "harder than"** and generates the player-facing order by **post-order DFS from the goal**, which is topologically sorted (a prerequisite is never introduced after its dependent) *and* front-loads achievable sub-goals. Its headline claim is the non-obvious one: **a good progression is goal-driven, not merely gradual** — it should build toward a hard task *as quickly as possible*, because "engagement often comes from a sense of accomplishment after completing hard tasks", which is why games have boss levels. Evaluated at n=248; synthesised progressions matched expert-designed ones on both engagement and learning.
- **[GRIP]** Rietveld, Miller & Kiverstein (2017), *The feeling of grip…* — 10.1007/s11229-017-1583-9.
- **[STIG]** Holland & Melhuish (1999), *Stigmergy, self-organization, and sorting…* — 10.1162/106454699568737.
- **[STIG-AD]** Salman, Garzón Ramos & Birattari (2024), *Automatic design of stigmergy-based behaviours…* — 10.1038/s44172-024-00175-7.
- **[PHERO-V]** Vectorial-pheromone navigation model (Robotics & Autonomous Systems, 2019) — 10.1016/j.robot.2019.103251. *(Grounds `RallyField`; confirm exact title.)*
- **[GOAP]** Orkin (2005), *Agent Architecture Considerations for Real-Time Planning in Games* — 10.1609/aiide.v1i1.18724.
- **[ECS]** Landyshev (2024), *The Role of Entity-Component-System Architecture…* — 10.52058/2695-1592-2024.
- **[ABM]** Antelmi et al. (2024), *Reliable and Efficient Agent-Based Modeling and Simulation* — 10.18564/jasss.5300.
- **[ME]** Mouret & Clune (2015), *Illuminating search spaces by mapping elites* — 10.48550/arXiv.1504.04909.
- **[QD]** Pugh, Soros & Stanley (2016), *Quality Diversity: A New Frontier…* — 10.3389/frobt.2016.00040.
- **[QD-PCG]** Yannakakis, Togelius, Liapis, Gravina & Khalifa (2019), *Procedural Content Generation through Quality Diversity* — 10.1109/cig.2019.8848053. *(Constrained Surprise Search.)*
- **[QD-OEE]** Faldor & Cully (2024), *Toward Artificial Open-Ended Evolution within Lenia using Quality-Diversity* — 10.48550/arXiv.2406.04235. *(Closest ref for the `mycelia/` continuous-CA system.)*
- **[PROB-ML]** Ghahramani (2015), *Probabilistic machine learning and artificial intelligence* (Nature) — 10.1038/nature14541. *(In corpus; empty title — identity inferred from content.)*
- **[BAYESOPT]** Bayesian optimization / integrated expected improvement — OpenAlex W2131241448. *(Title unindexed; content matches Snoek, Larochelle & Adams 2012 — verify before citing by name.)*
- **[UV-REV]** Kätsyri, Mäkäräinen, Förger & Takala (2015), *A review of empirical evidence on different uncanny valley hypotheses…* — 10.3389/fpsyg.2015.00390.
- **[UV-FMRI]** Cheetham, Suter & Jäncke (2011), *The human likeness dimension of the "uncanny valley hypothesis"…* — 10.3389/fnhum.2011.00126.
- **[TEST-OW]** Kato, Yoshida, Makihara & Inoue (2026), *Software Testing Beyond Closed Worlds: Open-World Games…* — 10.48550/arXiv.2604.04047.
- **[TEST-NT]** Patel & Hierons (2017), *A mapping study on testing non-testable systems* — 10.1007/s11219-017-9392-4.
- **[MISPERCEPT]** *Secrets and Misperceptions: The Creation of Self-Fulfilling Illusions* (Sociological Science, 2014) — 10.15195/v1.a26. **In corpus.** Pluralistic ignorance: a false belief survives because everyone assumes everyone else knows better. The mechanism behind FVS-O-5.
- **[EPISTEMIC]** Uncertainty representation — OpenAlex W3014596384 (2020). **In corpus; title field empty, identity inferred from content — verify before citing by name** (same caveat as [PROB-ML]/[BAYESOPT]). Carries the Fisher argument that a single probability distribution cannot represent *ignorance*: "not knowing the chance… and knowing the chance to be equal are two quite different states of knowledge." Grounds FVS-O-1's `Option`-not-`0.5` modelling.

---

## 7. Risks, decisions, and things to re-verify

- **Top design risk — kill-vs-capture fitness conflict (I-1).** The live QD objective rewards spectacular kills; making captures valuable pulls the opposite way. Unresolved, the director (H-3) surfaces anti-loop content. A weighting/decomposition decision is required, not optional — and I-1 gates H-3.
- **Determinism model is an unforced decision.** The known ARM↔x86 f32 divergence forces a choice (fixed-point core vs per-platform golden hashing) that C-6 (facing) and B-3/B-6 (field ops) make urgent. J-3's ARM lane is only meaningful once this is decided. Do not ship C-6 on divergent floats.
- **GOAP vs utility for squad orders is open.** Secure→Contain→Extract is naturally a GOAP precondition/effect plan ([GOAP]); creatures already run utility AI. Decide whether squad *orders* get a GOAP layer or stay utility considerations — affects P2, P3, P7.
- **Bevy 0.19 spellings to re-confirm on docs.rs:** observer trigger `On<Add, C>` (not `Trigger<OnAdd, C>`); lifecycle `Replace`→`Discard` (derive attribute `on_replace`→`on_discard`, though the trait *getter* may still read `on_replace`); `NextState::set_if_neq()`; `bevy_world_serialization` renames. Component hooks are the stable path for the reward invariant (B-4).
- **SCP canon to re-verify before shipping as copy:** the SCP-9191 origin line could not be re-confirmed verbatim in a full fetch (the article page returned mostly navigation chrome); the "alarm switch ON/OFF locker" containment detail *is* confirmed. Re-verify the 9191 origin against the live article.
- **Persistent operatives make death load-bearing (P10 + G-3).** Operatives now carry their knowledge across runs, so losing one is a permanent loss of everything they knew. That is the right stake, but it means **a squad wipe must be rare and legible**, not routine attrition — if wipes are common the meta-loop resets constantly and knowledge never compounds. Watch also for **veteran lock-in**: one operative accumulating everything while the rest rot. The counter-pressure is already in the design (fear accumulates alongside knowledge, so the veteran is also the most afraid), but it needs measuring once O-2 lands.
- **The O5 budget can death-spiral (P-1).** A performance-rated allowance means a bad run yields a small budget, which causes a worse run. Needs a floor and probably an explicit "displeased but not relieved of command" band. **Undecided** — safe to defer until the review exists, not safe to ship without.
- ~~**Asset conversion blocks the whole Site (N-10).**~~ **Retired 2026-07-27 — this risk never materialised and was already refuted in §3.** The Site shipped greyboxed from the 145 in-repo Kenney `.glb` and never touched the Ozea library. N-10 remains an art upgrade; it blocks nothing.
- **⚠️ TOP PROCESS RISK — "pure library, green tests, no caller" (found 2026-07-27).** Three subsystems shipped this way in one session: Push 4's research economy (FVS-E-5), the O5 economy (FVS-P-3), and operative beliefs (FVS-O-1b). All three are correct, well-grounded and fully unit-tested; none is reachable in play. FVS-B-8 caught the identical failure one push earlier and it still recurred, so treat it as **structural, not careless**: this repo's house style front-loads pure, harness-free logic — which is right, and is why the determinism story works — but green unit tests on a pure function then read as completeness. **The cheap counter is an acceptance test that drives the real `App`**, one per item, written *before* the item is called done. Every "Done when" in this backlog is phrased as player-observable behaviour for exactly this reason; the failure was not checking it.
- **The ASYNC aperture has never been looked at.** Its shader was authored blind and its uniform defaults are guesses (G-5). It is the game's signature image and the one thing in the Site that cannot be judged by a test.
- **Blind pattern-replaces break this codebase specifically.** One warning was "fixed" by a repo-wide rename that also hit a test genuinely using the binding it renamed: one warning became four compile errors. The tests here deliberately reuse production names to pin contracts, so a name is rarely as unique as it looks. Read the site, then edit it.
- **Scope realism.** The XL items (H-3, I-1, K-4, C-6) are correctly sequenced last and behind prerequisites. Do not let the appeal of the "full vision" pull them earlier than M4, or the M0–M3 foundation slips.

---

*Housekeeping: `[SDT-00]`, `[PROB-ML]`, `[BAYESOPT]`, `[EPISTEMIC]` and (added 2026-07-27) `[PROG]` have empty catalog `title` fields in home-still — running `catalog_backfill_title` would make them discoverable by title. Not required to build; noted for corpus hygiene. `[PROG]`'s identity is confirmed from the PDF's own ACM reference block, not inferred.*