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

**Dependency spine:** P1 → P2 → P3; P2 → P4 (research needs captured specimens); P4 ↔ P5 (unlock hooks ↔ tech-tree; persistence needs posteriors); P5 → P6 (director needs a retrained archive + resolved fitness); P6 → P7 (endgame needs the generator/difficulty spine). P8 is continuous with two items pulled early into P1/P2. P9 is continuous and independent.
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
- **FVS-C-3 — SCP-1048 out-watch capture** · M · *determinism: FixedUpdate; ATTENTION deposit/diffuse/decay in the deterministic field*
  1048 builds hostile copies while **unobserved**; its rule = keep the **ambient** `ATTENTION` field over its cell above threshold (out-watch), then device-capture. Uses the ambient decaying/diffusing scalar — **not** a per-entity watch boolean.
  *Done when:* sustained attention suppresses copy-building and enables capture; letting attention decay resumes building. · *Deps:* B-5 · *Touches:* `src/scp1048/`, `src/ai/field.rs` (ATTENTION), containment module · *Reading:* **[STIG]**
- **FVS-C-4 — SCP-150 parasite cure/extract** · M · *determinism: FixedUpdate*
  150 infects squad **and** crabs (rare three-body web); containment = cure/extract hosts via the host relationship (D-1) + single-target device.
  *Done when:* curing an infected host extracts the parasite as a specimen; untreated hosts stay infected. · *Deps:* B-5, D-1 · *Touches:* `src/parasite.rs`, containment module · *Reading:* [STIG-AD], [STIG], [ECS]
- **FVS-C-5 — Crab-nest source-elimination integration** · M · *determinism: FixedUpdate; swarm cadence deterministic*
  Connect nest-capping (B-7) to `src/nest.rs` breeding and the 40→5000 swarm.
  *Done when:* capping halts breeding; swarm attrition follows; secured flag set. · *Deps:* B-7 · *Touches:* `src/nest.rs` · *Reading:* [STIG-AD], [STIG]
- **FVS-D-1 — Parasite↔host relationship** · M
  Replace ad-hoc coupling with a custom `InfectedBy`/`Hosting` relationship pair.
  *Done when:* infecting sets the relationship; curing removes it; reverse traversal enumerates hosts. · *Deps:* — · *Touches:* `src/parasite.rs` · *Reading:* [ECS], [STIG-AD]
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

- **FVS-E-1 — `ResearchPosterior` component** · M · *determinism: seeded if in-sim*
  Belief over hidden params + a fog-of-war reveal bitset per specimen. "Research = epistemic action reducing uncertainty."
  *Done when:* posterior initializes at capture; reveal bitset starts empty; serializable (P5). · *Deps:* B-4, D-4 · *Touches:* research module, Site · *Reading:* **[PROB-ML]**, [GRIP]
- **FVS-E-2 — Experiment model + EIG selection** · L · *determinism: seeded*
  Rank experiments by Expected Information Gain (information-gain acquisition / posterior-entropy reduction) so the most informative is surfaced.
  *Done when:* per-experiment EIG computed and ordered; a test shows EIG picks the max-uncertainty-reduction experiment. · *Deps:* E-1 · *Touches:* research module · *Reading:* **[PROB-ML]**, [BAYESOPT]
- **FVS-E-3 — Reveal pacing (front-load resolvable surprise)** · M
  Pace the reveal so value tracks the **rate** of uncertainty reduction — front-load, don't drip.
  *Done when:* a completed arc reveals more early, tapering; tunable; test asserts monotone-decreasing default reveal rate. · *Deps:* E-1, E-2 · *Touches:* research module, UI · *Reading:* **[GRIP]**, [LPM]
- **FVS-E-4 — `Researched` marker + unlock hook** · S · *determinism: hook deterministic*
  When uncertainty crosses completion, add `Researched`; its `on_add` fires the unlock.
  *Done when:* completing research adds `Researched` and triggers exactly one unlock; idempotent. · *Deps:* E-1, F-1 · *Touches:* research module, tech-tree · *Reading:* [SDT-00], [ECS]
- **FVS-L-2 — Research/EIG HUD** · M · *determinism: render*
  Present candidate experiments ranked by EIG and the reveal as it front-loads.
  *Done when:* experiment list shows information value; reveals animate per the pacing curve. · *Deps:* E-2, E-3 · *Touches:* UI, research module · *Reading:* [PROB-ML], [GRIP]

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

- **FVS-G-1 — Persistent Site-67 entity** · M · *design: `docs/2026-07-26-site-hub-and-operative-knowledge.md` §2*
  The Site as a persistent root holding tech-tree flags + specimen relationships. **Exemption is free now:** FVS-A-5 made teardown `DespawnOnExit(RunState::Active)` via `session::run_scoped()`, so a Site entity persists simply by *not* carrying that tag — there is no exempt-list to maintain.
  *Done when:* the Site survives `RunState::Idle ↔ Active` round-trips; specimens accumulate across expeditions; run entities despawn around it. · *Deps:* A-5 (done) · *Touches:* new `src/site/`, `src/session/` · *Reading:* [ECS]
- **FVS-G-4 — Site-67 geometry + wings** · L · *design doc §2.1, §2.4*
  **Hand-authored, NOT generated** — a hub returned to every run must be learnable, and `Dungeon` is a single per-run resource so a second procedural world would be ambiguous. The Site is entities, not a `Dungeon`. Six areas: ASYNC door, containment wing, research wing, records office, requisition, briefing room.
  *Done when:* the Site renders as a navigable space with the six areas placed; the squad stands in it while `RunState::Idle`. · *Deps:* G-1 (**not** N-10 — see the correction in §3; greybox from the in-repo Kenney kit) · *Touches:* new `src/site/`, `assets/site/` · *Reading:* [ECS]
- **FVS-G-5 — The ASYNC door** · M · *design doc §2.2, §2.3*
  The portal onto the Backrooms, and the only way out. **No new state machinery** — it is a trigger volume that calls `NextState::set(RunState::Active)`, which FVS-A-5 already implements end-to-end. The *model* exists (`SM_DoorFrame_Double` — in `Pack_SciFi_A_002_V1.0`, **not** the B series as previously written; all 11 door meshes live there); what does not is the **aperture shader** — the volume inside the frame that visibly is-not-a-room. That is the game's signature image and squarely what this project already does well (17 authored `.wgsl`, the shared noise library, psi-vision/VHS).
  *Done when:* walking into the door starts an expedition; returning lands back at the Site; the aperture reads as anomalous-but-contained. · *Deps:* G-4 · *Touches:* `src/site/`, `assets/shaders/` · *Reading:* [UV-REV] (why generated space should read as *wrong*)
- **FVS-D-4 — Site↔specimen relationship + visible cells** · M
  Link the Site to each captured specimen with a Bevy relationship (the repo's third — see `squad::SquadRoster` and `containment::Holding` for the `Option<&Target>` gotcha), and **show it**: each specimen occupies a containment cell in the wing. `containment::Specimen` already exists and is already exempt from run teardown.
  *Done when:* the Site enumerates its specimens; each captured anomaly is visibly held in a cell; they survive teardown. · *Deps:* G-1, G-4 · *Touches:* `src/site/`, `src/containment/` · *Reading:* [ECS]
- **FVS-F-1 — Tech-tree flags resource + graph** · M
  A small resource of tech-tree flags + a data-authored unlock graph where each node is an anomaly-derived capability. The graph **is** the difficulty curriculum.
  *Done when:* graph parses; flags persist; a node unlocks only when prerequisites are met. · *Deps:* — · *Touches:* tech-tree module, config RON · *Reading:* [LPM], [SDT-00]
- **FVS-F-2 — Enabling (not numeric) unlock effects** · M
  Every unlock grants a **new verb / capability** ("999-derived morale field lets you calm 610"), never "+X%." Hard review rule.
  *Done when:* each unlock adds a capability/tool/containment option; a lint/checklist rejects numeric-only unlocks. · *Deps:* F-1 · *Touches:* tech-tree, equipment, containment · *Reading:* **[SDT-00]**, [SDT-13]
- **FVS-F-3 — Thaumiel dependency mapping to roster** · L
  Author the concrete curriculum: which captured anomaly unlocks the capability that makes the next-harder capture tractable.
  *Done when:* a playable path exists from first easy capture to a hard capture gated only by earlier unlocks. · *Deps:* F-1, C-1..C-5 · *Touches:* tech-tree, containment · *Reading:* [LPM], [QD-PCG], [SDT-00]
- **FVS-G-2 — Reflection-based save/load** · L · *determinism: `Reflect`-derived saved types*
  Persist Site/specimens/`ResearchPosterior`/tech-tree via `bevy_world_serialization` (or a reflection save crate); save the model subset, not view/aesthetic entities.
  *Done when:* save→reload restores Site, specimens, posteriors, unlocks; round-trip test passes. · *Deps:* G-1, E-1, F-1 · *Touches:* Site, research, tech-tree, save module · *Reading:* — (no corpus resource)
- **FVS-G-3 — Roguelite meta boundary** · M · *design doc §6*
  What persists vs resets. **Decided 2026-07-26: operatives PERSIST across runs, carrying their knowledge.** Persists: operatives + their beliefs, specimens, research, unlocks, filed reports, the O5 standing. Resets: the `RunSeed`'s world, run-scoped entities, the run clock/outcome.
  **Consequences of persistence that must be designed for, not inherited:** losing an operative is a permanent loss of everything they knew, so **death must be rare and legible** — routine attrition would reset the meta-loop constantly and knowledge would never compound. Reports become *insurance* (a voluntary hedge against your own death) rather than the only memory. Veterans diverge, which makes squad selection a real decision. **Watch for veteran lock-in** — one operative accumulating everything while the others rot; the natural counter-pressure is already in the design, since fear accumulates alongside knowledge and the veteran is also the most afraid.
  *Done when:* a lost run preserves meta-progress; a won run banks the extracted specimen; a dead operative's unwritten knowledge is gone. · *Deps:* G-1, A-1 (done) · *Touches:* `src/site/`, `src/session/` · *Reading:* [SDT-00]
- **FVS-P-1 — O5 performance review + budget** · M · *design doc §4*
  After each expedition the O5 Council issues an allowance rated on the Director's performance. **It reads the same metrics FVS-I-1's fitness must compute** — survivors, containment yield, time, breaches — deliberately: one source of truth for "how did that expedition go", surfaced twice (once to the search, once to the player).
  **Open:** the budget floor. A performance-rated allowance can death-spiral (bad run → small budget → worse run). Needs a floor and probably an explicit "the Council is displeased but you are not relieved of command" band. Safe to defer — the floor only matters once the review exists.
  *Done when:* an expedition produces a rating and an allowance; the rating is derived from the same terms the fitness uses. · *Deps:* G-4, I-1 · *Touches:* new `src/site/`, `src/squad_ai/` (shared metrics) · *Reading:* [SDT-00], [QD-PCG]
- **FVS-P-2 — Requisition: consumables only** · M · *design doc §4*
  Spend the O5 budget on medkits, ammo, field equipment. **Must NOT buy capabilities** — those come from research (F-2: unlocks grant verbs, never numbers). Keeping the two economies disjoint *by kind* is what stops the soft currency from eating the research loop.
  *Done when:* consumables are purchasable and carried into an expedition; a lint/checklist rejects any capability sold for budget. · *Deps:* P-1 · *Touches:* `src/site/`, equipment · *Reading:* **[SDT-00]**
- **FVS-L-3 — Site + tech-tree HUD** · M · *determinism: render*
  Show specimens, the Thaumiel graph, and locked/unlocked state.
  *Done when:* players navigate the curriculum graph and see prerequisites. · *Deps:* F-1, G-1 · *Touches:* UI, Site, tech-tree · *Reading:* [LPM], [SDT-00]

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
  TESTING.md invariant 11 explains the cost of), but it means H-1 is now a **prerequisite for any further
  QD work**, not a backlog item that can wait. Re-bake before trusting any elite overlay.
  *Done when:* retrained archive loads at current MODE_COUNT; smoke test shows non-degenerate policies. · *Deps:* — (blocks H-3) · *Touches:* `src/squad_ai/`, `bin/train.rs` · *Reading:* [ME], [QD]
- **FVS-H-2 — Make CMA-MAE emitter reachable (ideally)** · M · *determinism: offline*
  CMA-MAE is implemented + unit-tested but unreachable from any `train` subcommand (dead code). Expose it so the director samples a stronger archive; if skipped, the director is built on the weaker emitter — say so explicitly.
  *Done when:* a `train` subcommand runs CMA-MAE end-to-end and writes an archive. · *Deps:* — · *Touches:* `bin/train.rs`, emitter module, `coevolve.rs` · *Reading:* [ME], [QD], [QD-OEE]
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
- **FVS-N-8 — Gib spawn positions were order-dependent** · M · ✅ **FIXED 2026-07-26 — the cause was one line**
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
  **Corrects a standing claim in Push 8:** *"adding a resource is hash-neutral ⇒ no gameplay path keys
  off entity ids"* was inferred from the victory/timer path, which never spawns a gib. Registering
  `site::SitePlugin` (one bodiless `Startup` entity) was enough to perturb load timing and turn this
  latent bug into a hard failure of `both_terminal_paths_are_bit_reproducible` — which is how it was
  finally caught. · *Touches:* `src/autogib.rs`, `src/gore.rs` · *Reading:* [TEST-NT], [ABM]
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

- **FVS-O-1 — `Belief` model + firsthand acquisition** · M · *determinism: enters the pinned core*
  `Belief { subject, claim, confidence, provenance, acquired }` on every operative, plus firsthand acquisition (it happened to me). Deliberately distinct from `ai::brain::Fact`, which is *perception* ("is this true right now, sensorily"); this is *lore* ("I believe this about a class of thing, and here is where I got it").
  **Absence of a belief is NOT a low-confidence belief** — the one modelling point not to compromise. Fisher, quoted in [EPISTEMIC]: *"not knowing the chance of mutually exclusive events and knowing the chance to be equal are two quite different states of knowledge."* So an operative who has never met 1048 holds **no** `Belief` for it, not `confidence: 0.5`. `Option`, and "unknown" is a distinct behavioural state from "unsure".
  **Component discipline:** the belief set is a **value field** on a component present from spawn, never a marker toggled on acquisition — `scp1048`'s rule ("a flipped marker would split the hashed archetype"). Copy the `containment::Containment` pattern.
  *Done when:* an operative struck by 1048-A acquires a firsthand `Lethal` belief; a bystander does not. · *Deps:* — · *Touches:* new `src/knowledge/`, `src/squad.rs` · *Reading:* [EPISTEMIC], [ECS]
- **FVS-O-2 — Knowledge changes behaviour, both ways** · M · *determinism: modulates FEAR → hashed*
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
- **FVS-L-5 — Roster screen: what each operative believes** · M · *determinism: render*
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
- **Asset conversion blocks the whole Site (N-10).** 411 usable sci-fi meshes exist and none of them are `.glb`, which is the only format the game loads. Every Push 5 Site item is behind that one scripted batch job.
- **Scope realism.** The XL items (H-3, I-1, K-4, C-6) are correctly sequenced last and behind prerequisites. Do not let the appeal of the "full vision" pull them earlier than M4, or the M0–M3 foundation slips.

---

*Housekeeping: `[SDT-00]`, `[PROB-ML]`, and `[BAYESOPT]` have empty catalog `title` fields in home-still — running `catalog_backfill_title` would make them discoverable by title. Not required to build; noted for corpus hygiene.*