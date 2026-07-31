# BACKLOG.md — Foundation vs. Slop

> **Completed items live in `BACKLOG_ARCHIVE.md`** (split out 2026-07-30). This file lists only what is
> still open. IDs are never reused, so an ID absent here has either shipped or was never issued.

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

> **Milestone status, audited and then closed 2026-07-27.** M0 (P1) and M1 (P2) are **met**. M2 (P3) is
> met **except SCP-610**, deferred on the missing asset.
>
> **M3 is now met.** The morning audit found it was not — a great deal of M3 had shipped (Site-67, the
> ASYNC door, specimens visibly held in cells, save/load, and the whole research/economy/belief *logic*)
> but a player could capture and extract and come home and look at a specimen and then do **nothing**
> with it: no way to research it, no budget to spend, no beliefs carried. That was FVS-E-5, FVS-P-3 and
> FVS-O-1b, all landed the same day, plus F-3's curriculum and L-3's screen. The gate reads *"a full
> capture→research→unlock→harder-capture path playable across two persisted expeditions"*, and it is.
>
> M4 (P6/P7) is untouched and correctly sequenced behind I-1 → H-1.
>
> **Push 10 is complete as of 2026-07-27 (PR #67).** O-3, O-4 and O-5 all landed — beliefs propagate by
> conversation and decay along the chain, firsthand findings are filed at the Site and briefed to later
> squads, and a planted false report can be corrected both by experience and by curation. They did not
> in the end wait on K-3's content: O-3 *gave* `src/dialogue/` its job (every belief crossing the squad
> is voiced from an authored table), which was K-3's actual complaint. **The M3+ gate is met** — an
> operative who has met SCP-1048-A behaves differently from one who has only heard of it, and O-2's
> FEAR coupling is on rather than inert.
>
> **A reconciliation pass ran 2026-07-27 and it found more staleness than work.** Six items (A-4, F-2,
> I-2, F-3's content gap, O-2, E-6) were marked open while their acceptance had already been met in
> code — in one case the ⚠️ sat 55 lines above the config that closed it. One item was stale in the
> other direction: **FVS-P-3 claimed the O5 budget persisted and it did not**, which is the failure this
> backlog names as its top process risk, moved up a level — a claim of completeness in the entry itself,
> which no test contradicted because the round-trip test only asserted over the fields that existed.
> Read a status marker as a claim someone made on a particular day, and re-measure before trusting it.

**Dependency spine:** P1 → P2 → P3; P2 → P4 (research needs captured specimens); P4 ↔ P5 (unlock hooks ↔ tech-tree; persistence needs posteriors); P5 → P6 (director needs a retrained archive + resolved fitness); P6 → P7 (endgame needs the generator/difficulty spine). P8 is continuous with two items pulled early into P1/P2. P9 is continuous and independent.
**Wiring caveat added 2026-07-27:** the spine assumes an item's *logic* landing means the push advanced. Three items broke that assumption (E-5, P-3, O-1b). Read a "LANDED" marker as a claim about code, and the "Done when" as the claim about the game — they were not the same thing in Push 4, Push 5's economy, or Push 10.
**P10 (operative knowledge) is largely independent** — O-1/O-2 need only the existing drives and containment HUD, so it can run in parallel with P3/P4. Its later items converge: O-3 wants K-3's dialogue content, O-4 wants the Site (G-4) and save/load (G-2), and **O-5 is the payoff that ties the whole antagonist together** with K-4. **N-10 (asset conversion) does NOT block the Site — corrected 2026-07-26.** It blocks the *shipped-quality* Ozea art pass only. `assets/kenney_prototype-kit/Models/GLB format/` already holds **145 `.glb`** (walls, corners, four doorway variants, sliding doors, floors, columns, stairs, crates, indicators, floor buttons, signage numerals), licensed and in-repo. A **greybox** Site-67 is buildable today with zero conversion — and greyboxing first is correct for a hand-authored hub anyway: prove the layout and the loop, then spend the art budget on the meshes the Site actually uses.

---

## 4. The area pushes

**Site-67 and operative knowledge have a full design document** — `docs/2026-07-26-site-hub-and-operative-knowledge.md`. Push 5 and Push 10 items reference its sections; read it before starting either.

Each push lists a **goal**, the **vision tier** it serves, its **reading list** (keys resolve in §6; `[STIG]` etc.), and its items. Per item: a one-line description, **Done when** (acceptance), **Deps**, **Size** (S/M/L/XL), **Touches** (real modules), a **Determinism** flag, and **Reading**. `— (no corpus resource)` means an engine/design task with no honest corpus grounding; it is not an omission.

---

### Push 1 — Session Loop & Win/Lose  ·  Tier 2  ·  M0

> **All items in this push have shipped** — see `BACKLOG_ARCHIVE.md`. The goal and done-when above are kept as the record of what it was scoped to deliver.
**Goal:** make a session *resolve* — terminal states, per-run teardown, and a placeholder win — so the state machine is proven before any capture mechanic exists.
**Reading:** [TEST-OW], [ABM], [ECS]
**Done when:** a headless golden test drives a fixed seed to both Victory (placeholder timer) and Defeat (wipe) with exact-hash reproducibility, and a run can be *re-entered* — `QUIT TO TITLE` → `NEW RUN` yields a genuinely fresh world (A-5). The persistent-Site exemption is **not** in this push: it needs G-1, so A-4's remainder moves to P5.

> **Design correction (2026-07-25) — the win/lose decision cannot live in `AppState`.** `AppState` is registered only by `UiPlugin` in `lib::run`; `tests/replay.rs::ui_never_leaks_into_deterministic_core` *asserts it is absent* headless (`src/ui/state.rs:10-14`), so A-3's golden test as originally written was unimplementable. Resolved by splitting **decision** from **presentation**: a new harness-visible `src/session/` module owns `RunState` (`States`), a latched `RunOutcome` resource, `RunClock`, and the single `FixedUpdate` writer `resolve_run`; `AppState` keeps only the screens and mirrors the outcome. This is also the only shape under which A-5's run-scoped world construction can run headless.


---

### Push 2 — Containment Core  ·  Tier 1  ·  M1

> **All items in this push have shipped** — see `BACKLOG_ARCHIVE.md`. The goal and done-when above are kept as the record of what it was scoped to deliver.
**Goal:** the capture verb — a data-authored containment-rule model, the FixedUpdate system that runs it, the outcome hooks (kill-yields-nothing), and all **three archetypes** scaffolded. Swap A-3's placeholder for "extract one contained anomaly."
**Reading:** **[STIG]** (backbone — cite at header), [STIG-AD], [PHERO-V], [SDT-00], [ABM], [TEST-OW], [ECS]
**Done when:** an anomaly can be driven to `Contained` via a rule read against the fields; a kill produces zero specimen/research (asserted by test); the containment HUD reads why it's progressing/breaking.
**Note:** the three archetypes are genuinely distinct — do not pitch "one thrown sphere." Single-target *captures a body*; area-denial *bounds a region*; source-elimination *caps a structure* (which is honestly kill-for-no-specimen, not capture).


---

### Push 3 — The Anomaly Roster  ·  Tier 1  ·  M2
**Goal:** wire the roster that already has containment-shaped identity — **SCP-610** (zero code; needs an asset export first, see the C-1 correction), then 999/1048/150/crabs — each with its content and the swarm-behavior decisions. The roster leads with bespoke anomalies; **SCP-173/096 are deliberately deferred to Push 7** (the lore doc lists "leading with SCP-173" as an amateur tell, and they need new engineering, not this substrate).
**Reading:** [STIG], [STIG-AD], [UV-REV], [UV-FMRI], [ECS], [GOAP]
**Done when:** SCP-610 is capturable via quarantine and reads as a "slop" instance; 999/1048/150 capturable via their rules; crab infestation clearable via nest-capping; the two crab-behavior forks are decided and documented.

> **All items in this push have shipped** — see `BACKLOG_ARCHIVE.md`. FVS-K-1 (SCP-610's content/FX
> pass) landed 2026-07-30 and closed the last of them.

---

### Push 4 — Research Economy  ·  Tier 3  ·  M3

> **All items in this push have shipped** — see `BACKLOG_ARCHIVE.md`. The goal and done-when above are kept as the record of what it was scoped to deliver.
**Goal:** turn a captured specimen into knowledge — a belief over hidden parameters, max-information experiment selection, and a reveal paced to *feel* good. Grounded in Bayesian experimental design + the epistemic-action account of curiosity.
**Reading:** **[PROB-ML]**, [BAYESOPT], **[GRIP]**, [LPM], [SDT-00]
**Done when:** a captured anomaly's stat-sheet fog lifts through experiments ordered by expected information gain, front-loading resolvable reveals, and completing a posterior fires exactly one unlock.


---

### Push 5 — Site, Tech-Tree & Persistence  ·  Tier 3  ·  M3

> **All items in this push have shipped** — see `BACKLOG_ARCHIVE.md`. The goal and done-when above are kept as the record of what it was scoped to deliver.

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


---

### Push 6 — Adaptive Difficulty: QD Fitness & Live Director  ·  Tier 2  ·  M4 (large, gated)
**Goal:** the differentiator — a runtime archive selector that paces difficulty at the learning-progress band. This is a **new system**, not a rewiring of the static env-var `elite_overlay`, and it is **gated** behind a fitness that actually rewards captures (I-1) and a retrained, non-stale archive (H-1). Cluster I with H: they share the same QD papers, and **I-1 must land before H-3** or the director will surface anti-loop content.
**Reading:** **[QD-PCG]**, [QD], [ME], [QD-OEE], [LPM]
**Done when:** the retrained archive loads at MODE_COUNT 29; I-1's ablation shows capture-favoring seeds are selectable; successive expeditions receive archive-sampled challenges tuned toward intermediate difficulty, reproducibly.

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
  ⚠️ **The AUDIO archive is now stale for a third, structural reason (2026-07-30, FVS-K-1).**
  `audio_genome::N` grew **15 → 16** (`flesh_drone_loudness`, SCP-610's continuous acoustic
  stimulus). Archived genomes are fixed-length vectors, so the one bake that has ever landed —
  `elites_audio.ron`, 2026-07-19 — cannot decode and `is_feasible` rejects it loudly, as designed
  ("a stale archive is a re-train, not a resize"). Deliberately **not** re-baked at the time: this
  item is sequenced behind FVS-I-1, and baking now would optimise against an objective I-1 then
  invalidates — the exact mistake this entry already records one paragraph up.
  ⚠️ **And that knob's `BOUNDS` ceiling is a correctness constraint, not a range guess.** SCP-610's
  drone deposits `THREAT_ANOMALY` at the bloom's own position while its own containment rule caps
  that channel at 0.35 *there*, so a loud enough bloom is an uncontainable one — the search can
  delete a species' whole mechanic and be *rewarded* for it, because the fitness cannot see captures
  until I-1 lands. Pinned by `containment::the_loudest_evolvable_bloom_can_still_be_contained`.
  Re-check it if the ceiling, `scp610::DREAD_PER_DIN` or the authored threshold ever move.
  *Also expect:* `baseline_prior.ron` auto-re-sweeps on the first prior-backed search, because
  `ensure_prior_fresh` is mtime-driven and `config.ron` is newer.
  *Done when:* retrained archive loads at current MODE_COUNT; smoke test shows non-degenerate policies. · *Deps:* **I-1** (blocks H-3) · *Touches:* `src/squad_ai/`, `bin/train.rs` · *Reading:* [ME], [QD]
- **FVS-L-6 — The roster cannot be reviewed at the Site, and pretending it could was a CRASH (FOUND 2026-07-28)** · S
  **Reported from real play**, and reproduced: entering the Site after `RETURN TO SITE` panicked with
  `Parameter Res<State<MenuState>> failed validation: Resource does not exist`, from
  `knowledge::roster::toggle_roster`.
  **Root cause.** `MenuState` is a **SubState** sourced on `AppState::InGame`
  (`#[source(AppState = AppState::InGame)]`), so Bevy **removes** `State<MenuState>` the moment the app
  leaves `InGame`. `toggle_roster` takes it non-optionally and was registered
  `.run_if(in_state(AppState::InGame).or_else(in_state(AppState::Site)))` — so at the Site it ran with
  its own state gone.
  **The `.or_else(…Site)` was never a working feature.** Even without the panic the roster could not
  have opened there: `spawn_roster` hangs off `OnEnter(MenuState::Roster)`, and that state does not
  exist at the Site either. It bought a crash and nothing else.
  *Fixed:* restricted to `AppState::InGame` — where the whole `MenuState` mechanism actually works.
  **Deliberately NOT wrapped in `Option`**, which the Bevy error message suggests and which would be
  wrong here: it silences the panic and leaves a key that does nothing, which is a worse failure because
  it *looks* supported.
  *Pinned by* `replay::returning_to_the_site_after_a_run_does_not_panic` — it drives the real transition
  (`Debrief` → `RunState::Idle` + `AppState::Site`) under the **windowed plugin set**, and runs in the
  `test-harness` build so `bevy/debug` names the offending system. The shipped binary cannot.
  **What remains, and it is a real want:** reviewing what each operative believes *between* expeditions
  is exactly when it matters (FVS-L-5, FVS-G-3). It needs a **Site-side screen of its own**, the way
  `ui::site_hud` works — not a reach into the in-game overlay stack.
  *Done when:* the roster is openable at the Site through a Site-owned screen. · *Deps:* L-5 · *Touches:* `src/knowledge/roster.rs`, `src/ui/` · *Reading:* — (no corpus resource)
- **FVS-N-23 — The squad is 99% of the frame's geometry, and 23 materials of its draw calls** · L · *determinism: moves goldens* · ⛔ **DEMOTED 2026-07-30 — DO NOT DO THIS FOR PERFORMANCE**
  > **FVS-N-25 measured the frame as CPU-bound**: a 4x pixel cut moved frame time +4.1%. Decimating
  > these meshes would therefore buy ~nothing, while costing a golden re-pin and a
  > `valkyrie_asset.rs` contract re-pin. The census below is a **budget**, not a cause — treating it
  > as one was the mistake N-25 exists to have caught.
  > It stays open on **asset-hygiene** grounds only (`CLAUDE.md`: "all our assets should be
  > low-poly count"), at low priority, and it must not be cited as a performance fix unless a
  > loaded-scene A/B reverses N-25.
  **Reported from play as "it drops to 26 FPS", with the barrels suspected.** The barrels are innocent:
  **252 triangles each**, among the lightest assets shipped. Measured instead with the new
  `perf_probe` (`src/perf_probe.rs`) over a real session:
  * peak **visible** geometry `415,482` triangles;
  * five Valkyries account for `5 x 82,436 = 412,180` of them;
  * **everything else visible — 17 props, the dungeon, the lights — is 3,302 triangles.**

  So 99.2% of rendered geometry is the squad. But the triangle count is only half of it:
  `valkyrie.glb` carries **23 materials across 24 primitives**, and Bevy cannot batch across
  materials — that is **115 draw calls per frame for the squad alone**. Four materials hold 76k of
  the 82k triangles (`skin` 26,756 · `bodysuit` 24,436 · `gloves` 13,880 · `boots` 10,688); the other
  19 hold ~6k between them, including **five belt hooks of 12 triangles each, each with its own
  material**. That is a character authored for a cinematic close-up being drawn a few hundred pixels
  tall at the shipped iso zoom.
  **Two more offenders, from the player's own Ctrl+P captures (resident totals: 590,869 triangles
  across 554 primitives, tracked entities only — dungeon tiles are NOT counted):**

  | asset | instances | triangles | prims | tris/instance |
  |---|---:|---:|---:|---:|
  | `characters/valkyrie.glb` | 5 | 412,344 | 127 | 82,468 |
  | `scp150/scp-150.glb` | 5 | **86,410** | 35 | **17,282** |
  | `dimensional_crab.glb` | 29 | 66,026 | 63 | 2,276 |
  | `Wall Light.glb` | **120** | 7,200 | **240** | 60 |
  | every barrel variant | 17 | 4,284 | 17 | 252 |

  * **The mancae are the second-biggest cost in the game.** `scp-150.glb` is **17,282 triangles per
    instance** — for a small parasite, five of which spawn. Proportionally that is worse than the
    Valkyrie.
  * **The wall lights are the biggest DRAW-CALL cost.** 120 instances × 2 primitives = **240
    primitives for 7,200 triangles** — 43% of all primitives for 1.2% of the geometry. A 60-triangle
    prop split across two draw calls, placed 120 times.
  *Fix:* decimate the Valkyrie toward ~8k and atlas its materials to 3-4; decimate `scp-150`; merge
  the wall light's two primitives into one. Asset-side work in `scp_characters`, **not** an engine
  change. ⚠️ **Gated on FVS-N-25** — none of it is worth doing until the bottleneck side is known.
  ⚠️ Re-pins `tests/valkyrie_asset.rs`'s contract and **moves the goldens** — the squad mesh is pinned
  state, and swapping it re-perturbs the held-in seed calibration. Budget a measure-and-re-pin.
  *Done when:* the squad is no longer the dominant term in `vis_tris`, measured by the same probe on
  the same route. · *Deps:* — · *Touches:* `assets/characters/valkyrie.glb`, `tests/valkyrie_asset.rs` · *Reading:* — (no corpus resource)
- **FVS-N-24 — A 171 ms frame hitch that steady-state geometry does not explain (FOUND 2026-07-30)** · M
  Separate phenomenon from FVS-N-23 and **it will not be fixed by fixing that one**, which is the
  reason it is filed apart. The same trace shows a sustained 25-48 fps (a budget problem, N-23) *and*
  a **1% low of 8.7 fps with a worst frame of 171 ms** (a stall). A 171 ms hitch is not a triangle
  count; the candidates are asset streaming, a GPU sync, or the mycelia compute pass.
  *Investigate with:* `debug_screenshots/fps_trace.csv`'s `worst_ms` column against `t_secs` — a hitch
  that coincides with entering a new region points at streaming, one that recurs on a fixed period
  points at the compute pass. `--features bevy/trace_tracy` gives per-system attribution.
  ⚠️ **Also observed: a degradation over TIME at a fixed viewpoint.** Two captures 13 s apart from the
  identical camera position with identical resident geometry (590,869 tris) read 45.5 fps / 134 ms
  worst, then 27.3 fps / 224 ms worst. Whatever this is, it is not geometry and not location.
  > ### 📐 It is not a degradation — it is a ~11.8 s OSCILLATION. Measured 2026-07-30 from `fps_trace.csv`.
  > The two captures above sampled opposite phases of a cycle, which is why it read as decay. Over the
  > 44 s trace, `fps_local` swings between a **slow phase (mean 68.1 fps)** and a **fast phase (mean
  > 146.0 fps)** — a **2.14×** swing — with slow-phase onsets at **4.6 s, 16.2 s, 28.4 s, 40.0 s**:
  > intervals 11.6 / 12.2 / 11.6, **mean period 11.8 s over 3 clean cycles**. Slow phase ~4.6 s of each.
  >
  > **The scene is provably constant across the whole window** — same cell (80,112), same region 13,
  > same biome, `vis_units` 5, `vis_hostiles` 0, `vis_props` 7, `vis_lights` 7, and `vis_tris` varying by
  > **4 out of 413,364**. So this is neither geometry nor location nor entity count, and N-23 cannot
  > touch it.
  >
  > **Host CPU frequency scaling is ruled out — by magnitude, not by period.** Sampled `/proc/cpuinfo`
  > at 2 Hz for 42 s under sustained load: mean-of-core-means 4132 MHz, range 3639–4200, a **1.15×**
  > swing. A 15% clock change cannot produce a 114% frame-time change even if perfectly correlated.
  > *Whatever this is, it is in the application.*
  >
  > **Next step is per-system attribution, not more grepping.** `--features bevy/trace_tracy` over a
  > fixed-camera run ≥ 3 cycles (≥ 40 s), then find the system whose cost has an 11.8 s period. Cheap
  > ablation if Tracy is inconvenient: re-measure the same fixed camera with the mycelia plugin out —
  > it owns the only GPU compute pass in the frame, which is the standing suspect for a periodic cost.
  > *Ruled out already:* `MIN_APPEARANCE_RAMP_SECS = 12.0` (`mycelia/perceptual.rs:68`) is tantalisingly
  > close to the period but is a `slew` **rate limiter**, not a periodic trigger — it produces a smooth
  > ramp, not a cycle. `perf_probe`'s `HOTSPOT_EVERY_SECS = 10.0` is the wrong period and is one small
  > file write.
  > **Note the prior suspect does not fit.** FVS-N-13 leaks one dungeon *per expedition*; this cycles
  > three times inside a single run at a fixed camera. N-13 is still a real leak — it is just not this.
  *Done when:* the stall is named, and either fixed or shown to be unavoidable with the measurement
  behind it. · *Deps:* N-25 · *Reading:* [ABM]
- **FVS-N-25 — Establish whether the game is CPU- or GPU-bound BEFORE optimising either (GATES N-23/N-24)** · S · ✅ **ANSWERED 2026-07-30: CPU-BOUND**
  > **Measured.** Identical scene and seed at two pixel counts (`FVS_WINDOW`, `FVS_AUTORUN`, vsync off,
  > first 10 s discarded, 68 samples each):
  >
  > | run | pixels | mean fps | frame time | visible tris |
  > |---|---:|---:|---:|---:|
  > | full | 2.48 Mpx | 117.8 | **9.94 ms** | 413,364 |
  > | half | 0.62 Mpx | 113.6 | **10.35 ms** | 413,364 |
  >
  > **A 4x cut in pixels moved frame time by +4.1% — i.e. not at all** (the half-res run was
  > marginally *slower*, which is noise). At ~413k triangles on screen the renderer is not the
  > constraint. **FVS-N-23's mesh decimation would buy approximately nothing**, and it would have
  > cost a golden re-pin and a `valkyrie_asset.rs` re-pin to find that out.
  >
  > ⚠️ **The first attempt at this test LIED, and the failure is worth keeping.** It reported
  > CPU-bound at 16.75 vs 16.82 ms — because both runs sat at exactly **60.0 fps median**, i.e.
  > both were vsync-capped. A capped frame time measures the display, not the renderer, and two
  > capped runs can only ever report "no difference". Measurement mode now forces
  > `PresentMode::AutoNoVsync`. **Check the median for a suspiciously round cap before believing
  > any frame-time comparison.**
  >
  > ⚠️ **Scope of the claim, stated precisely:** the probe run held ~118 fps where the player saw
  > 26-45, with the same geometry but no live swarm (29 crabs, 5 mancae) and immature mycelia. So
  > this establishes the **renderer is not the constraint at that geometry**. It does *not* say
  > which CPU system eats the frame when the swarm is live. Re-run the same A/B on a LOADED scene
  > before extending the conclusion.
  > *Next:* `docs/perf_improvements_plan.md` aims at the CPU side and is therefore aimed correctly;
  > `--features bevy/trace_tracy` for per-system attribution.
  **This is not yet known, and both of the obvious plans assume opposite answers.** FVS-N-23 measured
  a lopsided *geometry* budget (99% of visible triangles are the squad; 554 primitives resident) and
  concludes "decimate the assets". `docs/perf_improvements_plan.md` measured a lopsided *CPU* budget
  (~48M `is_floor` calls/sec in the stigmergy diffusion stencil) and concludes "precompute the
  neighbour table". **Both cannot be the bottleneck, and doing the wrong one first buys nothing** —
  decimating meshes on a CPU-bound frame changes no number at all.
  What the `perf_probe` measures is *frame time*, which is agnostic between them; the triangle and
  primitive counts describe a budget, not a cause. Saying otherwise is reading a correlation into a
  census.
  *Cheapest decisive experiments, in order:*
  1. **Halve the window resolution and re-measure the same route.** Frame time unchanged ⇒ CPU-bound;
     frame time improves roughly with pixel count ⇒ GPU-bound. One run, no code.
  2. `--features bevy/trace_tracy` for per-system attribution — the heavy sim systems already carry
     `info_span!`s for exactly this.
  3. Toggle `MyceliaPlugin` off and re-measure (it is a GPU compute pass, so it discriminates too).
  ⚠️ Note the two captures taken **13 s apart from the identical camera position** with identical
  resident geometry read **45.5 fps and 27.3 fps** (worst frame 134 ms then 224 ms). A degradation at
  a fixed viewpoint with fixed geometry is not a geometry problem *at all* — it is time-dependent, and
  FVS-N-13 (every expedition leaks a whole dungeon, tiles + Avian colliders, uncounted by the probe)
  is the standing candidate.
  *Done when:* the bound is named with the measurement that shows it, and N-23/N-24 are re-ordered
  behind that answer. · *Deps:* — · *Touches:* — · *Reading:* [ABM]
- **FVS-I-6 — Audit descriptors BEFORE adding any of I-7..I-10 (PREREQUISITE)** · M · *determinism: offline* · ✅ **STATIC AUDIT RUN 2026-07-30 → `docs/descriptor_audit.md`**
  > **Read the audit before touching I-7..I-10 — it changes all four.** Headlines:
  > * **I-10 is STALE** — `BreedingTuning` (7) and `ParasiteTuning` (14) are *already* decoded in
  >   `world_genome.rs:452/490`, `respawn_interval` and `manca_count_max` included. Closed, archived.
  > * **I-9 names the wrong file.** `src/ai/tuning.rs` is fully encoded already; the unevolved knobs are
  >   `src/behavior_tuning.rs::PerceptionTuning`, of which `behavior_genome` covers only 2 of 13.
  > * **I-8 fails** — all 15 `MetropolisWeights` knobs tune *arrangement*, the level archive bins on
  >   *count* × mould. Needs your remove/add-axis/couple call.
  > * **I-7 passes for a subset only** — ~8 sim-relevant knobs reach `deaths`; the ~22 cosmetic ones are
  >   textbook N-21.
  > ⚠️ **And one thing the audit surfaced that nobody chose:** `behavior_genome` (89 knobs, *squad*
  > tuning) and `audio_genome` (16 knobs, acoustics) are **both** binned on `swarm_descriptor` — the
  > *swarm's* aggression × persistence. The only bake that has ever landed on those axes filled **3 of 64
  > cells**. Whether that descriptor is right for either population is a live question, and a descriptor
  > choice is yours. It gates I-9.
  > *Still unmeasured:* every "moves an axis" claim is a code-path argument. The settling test is an
  > ablation per knob group — a search run, not a read.
  The four items below add ~20 knobs to the genomes. **Adding a knob no descriptor can see makes the
  archive worse, not better** — two genomes differing only in that knob land in the same cell, and the
  winner is decided by evaluation luck. That is not a hypothetical: it is exactly FVS-N-21 (biome
  genes against a level descriptor whose only axes are `furniture_per_room` and `infestation`), and it
  is the mechanism that collapsed the policy archive once already.
  *For each knob group, establish which descriptor axis moves when it moves.* If none does, decide
  **remove / add-axis / couple** before landing the gene, using N-21's three-way framing.
  ⚠️ Every genome length change also invalidates that population's baked archive — `audio_genome`
  already went 15 -> 16 for FVS-K-1, and that re-bake is parked under H-1. Do not multiply that debt
  without deciding it is worth paying. · *Deps:* — · *Reading:* **[QD]**, [ME], [QD-PCG]
- **FVS-I-7 — Gore settings (6 knobs) are unevolved** · M · *determinism: offline*
  Not in any genome, yet **a gore knob already tipped a 5/5 win into a wipe** — so it is a live
  difficulty dial the offline search cannot see, which is precisely what `CLAUDE.md`'s "every feature
  must evolve" rule exists to prevent.
  > ✅ **Passes the audit — for a SUBSET only (2026-07-30).** `GoreSettings` is ~30 knobs and most are
  > cosmetic (`spray_color_a/b`, `pool_color`, `pool_gloss`, `dry_time`, `wall_splat_size`, ...);
  > encoding those is textbook N-21. Encode **only** the ~8 with a causal path to the world archive's
  > `deaths` axis: `max_gibs`, `chunk_restitution`, `gib_friction`, `autogib_pieces_base`,
  > `autogib_min_pieces`, `autogib_max_pieces`, `autogib_speed_mult`, `meat_count`. Say in the code why
  > the cosmetics are excluded, so nobody "completes" the group later.
  > The header count says "6 knobs"; the real sim-relevant count is ~8. Fix that when landing.
  · *Deps:* **I-6** · *Touches:* `src/gore.rs`, `src/squad_ai/world_genome.rs`
- **FVS-I-8 — `MetropolisWeights` (10+ knobs) are unevolved** · L · *determinism: offline*
  The largest of the four, and the one most likely to fail I-6's audit: it shapes the *level*, and the
  level descriptor has two axes. Run the audit before writing any encode/decode.
  > ⛔ **IT FAILED THE AUDIT (2026-07-30).** All 15 knobs are either sampler settings (`iterations`,
  > `temp_start/end`, `translate_sigma`, `rotate_prob`) or arrangement-quality weights (`w_overlap`,
  > `w_wall`, `w_facing`, `w_group`, `coherence`, ...). The level archive bins on **clutter ×
  > infestation** — clutter is `furniture_per_room`, a *count* set upstream by the grammar, and
  > Metropolis decides *where* pieces go, never *how many*. **Zero of 15 move an axis.** This is N-21 at
  > 15x scale.
  > **Your call, three ways:** (1) **remove** — leave arrangement authored, cheapest, loses nothing the
  > archive can reward; (2) **add an axis** — an arrangement-coherence descriptor, but that is a
  > descriptor change and invalidates the level archive; (3) **couple** — encode only `coherence` and
  > accept it rides fitness, not a descriptor.
  · *Deps:* **I-6** · *Touches:* `src/placement/`, `src/squad_ai/level_genome.rs`
- **FVS-I-9 — Perception tuning is unevolved** · M · *determinism: offline* · ⚠️ **RESCOPED 2026-07-30 by the I-6 audit**
  What agents can sense is a direct difficulty axis and is currently authored-only.
  > **The original *Touches* was wrong.** `src/ai/tuning.rs` is `AiTuning` — the 27 field-propagation
  > knobs at the head of `world_genome`'s `BOUNDS`, i.e. **already fully evolved**. The genuinely
  > unevolved knobs are `src/behavior_tuning.rs::PerceptionTuning`, of which `behavior_genome` encodes
  > only 2 of 13 (`leash`, `squad_think_interval`). The 11 that are missing: `examine_sight(_release)`,
  > `threat_sight(_release)`, `psi_sight(_release)`, `ward_sight(_release)`, `wounded_frac(_release)`,
  > `leash_in`.
  > **Blocked on the descriptor question, not just on I-6.** These are *squad* knobs and the behaviour
  > archive bins on the *swarm's* aggression × persistence — second-order, which is how N-21 happens.
  > **Encode constraint:** each `_sight`/`_sight_release` is a Schmitt band; `decode` must enforce
  > `release >= sight` or the search produces chattering perception no descriptor can explain.
  · *Deps:* **I-6** (+ the descriptor call) · *Touches:* `src/behavior_tuning.rs`, `src/squad_ai/behavior_genome.rs`
- **FVS-B-10 — Give the acoustic channels a player-facing payoff (stealth / noise discipline)** · L
  `NOISE_SQUAD`/`NOISE_SWARM` propagate and are *perceived* (`unit_fear_of_din`, `crab_fear_of_din`,
  `investigate_threshold`) but no player-facing verb reads them, so the whole acoustic layer is
  machinery without a game attached.
  **FVS-K-1 paid the first instalment**, which is why this is now a generalisation rather than a
  greenfield design: SCP-610's containment rule caps `NOISE_SQUAD` at 0.20 — which is what finally
  makes the existing `HOLD FIRE` verb load-bearing — and its drone deposits into `NOISE_SWARM`. So
  both channels have exactly one consumer each and a proven shape to copy.
  *Wanted:* movement/fire/verb choices that trade speed for quiet, and a HUD channel that makes the
  din legible (the containment HUD already names channels, so the vocabulary exists).
  · *Deps:* — · *Touches:* `src/audio_tuning.rs`, `src/squad.rs`, `src/ui/` · *Reading:* **[STIG]**, [STIG-AD], [PHERO-V]
- **FVS-N-21 — The biome genes are invisible to the level descriptor (FOUND 2026-07-30, audit)** · S
  Q-3 added `biome_mix`/`biome_scale` to `LevelGenome` because `CLAUDE.md` requires wiring features into RL/QD. **Audit says the descriptor cannot see them.** `level_quality.rs:72` has exactly two axes — `furniture_per_room / 8` and `infestation / 0.5` — and biome moves neither: furniture keys on room *tags*, mould affinity keys on room *tags*, and `score()` never reads biome. Two genomes differing only in biome land in the same cell with the same fitness, so the winner is decided by whatever else differs, or by evaluation luck — which is materially worse while N-13 is live. This is the archive-collapse mechanism that already bit the policy archive once.
  **Three ways out:** remove the genes (biome is an authored art choice, and `docs/animation.md` already establishes cosmetic-only systems as a documented exception); add a third descriptor axis (the archive is 2-D — expensive for a cosmetic dial); or **couple biome to something the descriptor already measures** — concrete resists mould, carpet harbours it — which makes `biome_mix` move the `infestation` axis with no archive change and is lore-plausible. Leaving it as-is is the one option to avoid. · *Deps:* — · *Reading:* [QD]
- **FVS-N-22 — Appending a `knowledge::Subject` invalidates every campaign save (FOUND 2026-07-30)** · S
  C-1's `Subject::Flesh` broke save loading: `persist` refuses with *"Expected an array of length 7 but found 6"*. The refusal is **correct** — misreading saved beliefs would be worse — but every existing campaign breaks on any content addition, and the failure cascades in a way that cost real time here: deleting the save reset `ConversationsPlayed`, so the one-shot intro replayed every launch, and its `Choice` node froze the sim indefinitely (`dialogue/runtime.rs:4`), which made **every screenshot taken that day a capture of a paused game**. Needs a ruling: accept as normal for content additions, or version the save and migrate.
- **FVS-N-13 — Dungeon tiles are not `run_scoped()`, so every expedition leaks a whole dungeon (FOUND 2026-07-28, review)** · M · *determinism: touches the pinned core*
  `dungeon::render::spawn_tiles` moved from `Startup` to `OnEnter(RunState::Active)` in the per-run
  migration, but its tile entities never gained `session::run_scoped()`. The only `run_scoped()` in the
  file is on the ground half-space. So floor tiles, wall slabs, lintels, corner posts — **and their
  Avian static colliders** — survive the run.
  **Play run 1 → RETURN TO SITE → run 2** and a different map generates at the same origin while run 1's
  entire tile set is still resident: two interpenetrating dungeons, invisible run-1 walls that gib
  chunks bounce off, and an unbounded entity/mesh/collider leak of one dungeon per expedition.
  `session::run_scoped`'s own doc names dungeon tiles as a carrier; the migration simply missed them.
  `tests/session.rs::leaving_and_re_entering_a_run_builds_a_fresh_different_world` only counts `Unit`,
  which is why nothing caught it.
  ⚠️ **Adding the tag WILL move the goldens** — it changes what exists in the pinned world — so this
  needs a deliberate measure-and-re-pin, not a drive-by edit. That is the only reason it is filed rather
  than fixed. · *Deps:* — · *Touches:* `src/dungeon/render.rs`, `src/gore.rs`, `src/mycelia/` · *Reading:* [ECS]
- **FVS-H-8 — FVS-H-3's director is INERT: the elite overlay writes config nobody re-reads (FOUND 2026-07-28, review)** · M
  `director::pick_next_challenge` calls `apply_dim(&mut gc, Dim::Levels, …)`, which writes `gc.dungeon`,
  `gc.mycelia`, `gc.placement.metropolis` and `gc.placement.density`. **None is ever read again.**
  `DungeonPlugin::build` copies `gc.dungeon` into `DungeonConfigRes` and `generate_dungeon` reads only
  that; `PlacementPlugin` and `MyceliaPlugin` snapshot theirs the same way at plugin-build time.
  So the log says a challenge was sampled and **every expedition is identical**. FVS-H-3 ships a
  correct, tested selector wired to nothing — the exact "pure library, no caller" shape this backlog
  names as its top process risk, one layer out.
  *Fix is not in `director.rs`:* either the consumers must read `GameConfig` at world-build time, or the
  director must write the resources they actually read (`DungeonConfigRes`, `PlacementSolvers`,
  `Density`, `MyceliaConfig`). The second is smaller; the first is more honest about where config lives.
  **An architectural call, which is why it is filed.** · *Deps:* H-3 · *Touches:* `src/director.rs`, `src/dungeon/mod.rs`, `src/placement/`, `src/mycelia/`
- **FVS-I-5 — `containment_criterion` still gates the squad and swarm archives (FOUND 2026-07-28, review)** · M · *determinism: offline*
  FVS-I-1's constraint was moved out of the shared `minimal_criterion` and into `coevolve/search.rs` —
  but it landed inside `score_triple_compact`, whose `None` **discards the whole triple**. So a
  capture-hostile world drops the squad and swarm candidates evaluated alongside it, which is precisely
  the coupling the scoping fix existed to remove. The constraint is correct; its *placement* still is
  not. Correcting it means letting the world candidate be rejected independently of its partners, which
  changes what the co-evolution admits — so it wants a probe run, not a quick edit. · *Deps:* I-1 · *Touches:* `src/squad_ai/coevolve/search.rs`
- **FVS-J-7 — The config mtime guard rejects `train apply`, the one process meant to rewrite config (FOUND 2026-07-28, review)** · S
  `config::CONFIG_FINGERPRINT` errors if `config.ron`'s mtime changes mid-process — a good guard against
  editing config during a test run. But `train apply` **writes** `config.ron` and then reloads it to
  verify, so it trips its own guard and aborts **with the file already rewritten**, which is the
  half-baked state the guard exists to prevent. · *Deps:* — · *Touches:* `src/config.rs`, `src/bin/train.rs`
- **FVS-J-8 — `repin_one` cannot re-pin a per-platform golden (FOUND 2026-07-28, review)** · S
  `bake::repin_one` refuses a marker that appears twice, treating duplication as ambiguity. The
  per-platform golden decision made `GOLDEN`/`GOLDEN_FIELD` `cfg(target_arch)`-selected, so each marker
  now **literally appears twice** in `tests/replay.rs`. `train apply --repin-goldens` therefore fails at
  the re-pin step every time. Two correct answers land in one file and the tool calls it ambiguous.
  · *Deps:* J-3 · *Touches:* `src/bake.rs`
- **FVS-J-6 — Rollout determinism breaks under CI-grade contention, and this box cannot reproduce it (FOUND 2026-07-28)** · M · *determinism: THE core invariant*
  ⚠️ **Do not close this as a flake.** Non-determinism *is* intermittent; a test that detects it fails
  intermittently **because the bug is intermittent**. That is the test working.
  **The evidence.** On PR #68, `search_rollouts_of_mutants_are_reproducible_under_load` failed on CI:
  ```
  mutant #3 (rng seed 0x6d07a17) on world 0x5c09191:
    2 distinct [(66c73bd35b9ceb48, 1f25), (15c4e253ae10b46a, 1f25)]
  ```
  Two replicate rollouts of the **same** mutant genome on the **same** world: snapshot hashes differ,
  **field hashes identical**. So actor state diverged while the stigmergy grids did not — which points
  at a gameplay decision keyed on ECS query order rather than at a field-update ordering bug.
  **It is intermittent, and that is measured rather than assumed:** commit `1d3c0a7` produced a harness
  lane **success and a failure on two runs of identical code**.
  > ### 🔑 THE DIVERGENCE IS BIMODAL AND STABLE — this is a discrete decision, not float drift
  > It reproduced at `08f7b38` with **byte-identical output** to the `1d3c0a7` failure: same mutant #3,
  > same world `0x5c09191`, and the *same two* snapshot hashes `66c73bd35b9ceb48` / `15c4e253ae10b46a`.
  > Two different commits, two different CI runs, one pair of outcomes.
  >
  > That rules out the obvious explanation. **Accumulated float noise under scheduling pressure would
  > give a different hash every time.** A rollout landing on exactly one of *two* states, repeatably,
  > is a **binary decision flipping** — an `if`, a tie-break, a "first match wins" over an unordered
  > iteration — not drift.
  >
  > **And the field hash is `1f25` in BOTH branches.** The stigmergy grids evolve identically; only
  > actor state (`Transform`/`Health`) differs. So whatever flipped either does not deposit, or flips
  > late enough that no deposit follows it. That is a strong filter on where to look: a decision that
  > moves or damages an actor without writing a field.
  >
  > This is far more tractable than the failure first appeared, and it narrows the search to a
  > *specific* genome × world pair that sits on a knife edge — which is exactly what
  > `evaluate::trace_episode` + `row_trace` are built to bisect.
  **It is very likely NOT new.** main's scheduled nightly the same day failed *both*
  `search_parallel` tests — the same family (load-based determinism), on the same runner class, on a
  branch containing none of PR #68's work. Two independent tests in that family failing on CI while
  passing on a 24-core workstation is one pattern, not two.
  **Why this box cannot see it, and why that matters.** The load generator spawns a fixed **8 busy-loop
  threads**. On 24 cores that is mild; on a 2–4 core GitHub runner it is 2–4× oversubscription. **The CI
  runner is a strictly harsher determinism probe than the development machine** — which inverts the
  usual assumption that local reproduction is the prerequisite for investigating. TESTING.md invariant
  13 applies directly: an exoneration is only as strong as the condition it was measured under, and
  every local pass here was measured under a weaker one.
  ⚠️ **FVS-J-5's lane split reduced this test's visibility, and that was decided BEFORE this failure was
  seen.** It now runs only in the nightly job — still a HARD job, so it still gates, but no longer per
  PR. That split was justified on runtime (two tests were ~89% of a 2-hour lane) and it stands on those
  grounds; it must not become the reason this goes uninvestigated. **If this proves to be a live
  determinism bug, the split should be revisited** — a per-PR lane that cannot catch it is the exact
  gap FVS-J-5 exists to close.
  **Data point 2026-07-30 (FVS-K-1), recorded because it is evidence and NOT because it exonerates
  anything.** `session::the_wipe_paths_actors_and_fields_are_reproducible_under_load` — same family,
  different test — failed **once**, during a sequential full-harness run with unrelated shell work
  happening alongside it. It then passed **6/6** on re-runs: 4 idle, and 2 under a deliberate
  14-busy-loop contention that was verifiably biting (runtime 55–77 s idle → 114–128 s loaded). The
  full `session` target is 21/21.
  Per this entry's own standing instruction, **that is not a clean bill of health** — an intermittent
  detector failing intermittently is the detector working, and every one of those 6 passes was
  measured under a *weaker* condition than the one that failed. It is logged so the next occurrence
  has a predecessor to compare against: the distinguishing question is whether the divergence is
  again **bimodal** (a flipped discrete decision) or a fresh hash each time (float drift).
  *Investigate with the tools the failure message names:* `evaluate::trace_episode` on the printed
  `(mutant, world)` pair — it folds snapshot + field + gib hashes — then `evaluate::row_trace` at the
  first divergent tick, **multiset** diff (a set difference lies when tied actors share a row). The
  precedent is `docs/rl/2026-07-16-search-rollout-nondeterminism.md`, the G0 investigation.
  **Reproducing it locally will likely require raising the load generator's thread count** well past 8,
  to emulate the runner's oversubscription — the cheapest first experiment, and it is one this box can
  actually run.
  *Done when:* the divergent decision is named and fixed, **or** the failure is proven to be an artefact
  of oversubscription that cannot occur at shipped thread counts — with the measurement that shows it.
  · *Deps:* — · *Touches:* `src/squad_ai/evaluate.rs`, wherever the order-dependent decision lives · *Reading:* [ABM], [TEST-NT]
- **FVS-H-4 — SPIKE: is ABSOLUTE competence progress right, or should it be signed?** · S · *determinism: offline measurement*
  **The decision, made 2026-07-28 in FVS-H-3.** `CellHistory::interest` takes `|progress|`, so a cell the
  player is getting rapidly **worse** at is as interesting as one they are mastering — both mean the
  difficulty is live rather than settled. Signed progress would make the director *flee* anything going
  badly, which is the opposite of a curriculum.
  **Why it is a spike and not a settled fact:** [LPM] defines CPM over the *derivative* of competence
  without committing to a sign, because a robot setting its own goals and a game pacing a player are not
  the same problem. A player on a losing streak may experience "the game keeps sending me back to the
  thing beating me" as punishment rather than as a curriculum — the failure mode absolute progress
  invites and signed progress cannot have.
  *Falsify it:* play (or replay) a campaign that deliberately declines, and check whether the director
  parks in the declining cell. Measure how many consecutive expeditions it takes to leave.
  *If wrong:* half-wave rectify — weight gains fully and losses partially — rather than flipping to
  pure signed, which would reintroduce the flee behaviour. · *Deps:* H-3 · *Reading:* **[LPM]**, [GRIP]
- **FVS-H-5 — SPIKE: does `UNVISITED = INFINITY` starve the measured cells?** · S · *determinism: offline measurement*
  **The decision.** An unvisited cell scores `f32::INFINITY`, so every cell is tried once before any
  measured cell is revisited. Without it a pure-progress rule can never choose a cell with no history —
  there is no progress to measure — and the campaign never leaves where it started. [LPM]'s progress
  niches have to be *discovered*.
  **The risk it creates, and it is real:** the shipped `elites_levels.ron` has **55 occupied cells**. At
  one expedition per pick that is 55 expeditions of pure exploration before the director exploits
  anything it learned — which may be longer than an entire campaign. Optimism under uncertainty is
  correct in principle and possibly far too patient at this archive size.
  *Falsify it:* count expeditions-to-first-revisit on a real campaign against expected campaign length.
  *If wrong:* the standard fixes are a decaying optimistic prior (finite, not `INFINITY`) or sampling a
  *subset* of cells per campaign. Prefer the first — it keeps one mechanism. · *Deps:* H-3 · *Reading:* **[LPM]**, [QD]
- **FVS-H-6 — SPIKE: `Option` vs `0.0` for an unmeasured cell — is the distinction load-bearing?** · S
  **The decision.** `CellHistory::learning_progress` returns `Option<f32>`, `None` until two full
  windows exist — *not* `0.0`. "No evidence" and "measured flat" are different states, and collapsing
  them makes an untried cell look **mastered**, which is the one reading that would stop the director
  exploring entirely. Same distinction `knowledge::Knowledge::of` draws for beliefs, grounded in
  [EPISTEMIC]'s Fisher argument that ignorance is not a uniform probability.
  **Why it is still a spike:** the distinction is currently load-bearing only because `interest()` maps
  `None` to `UNVISITED`. If FVS-H-5 replaces `INFINITY` with a finite prior, the `Option` may collapse
  into that prior and become ceremony. Worth re-checking *after* H-5, not before.
  *Falsify it:* after H-5 lands, try `learning_progress() -> f32` with the prior folded in and see
  whether any behaviour changes. If nothing does, simplify. · *Deps:* H-3, H-5 · *Reading:* [EPISTEMIC], **[LPM]**
- **FVS-H-7 — SPIKE: is "no archive → the authored world" genuinely one path?** · S
  **The decision.** A missing or empty archive makes `CurriculumDirector::pick` return `None`, the
  director does not fire, and the **authored** `config.ron` world plays. Claimed as one path rather than
  a fallback: with nothing to sample there is no degraded substitute being written — the authored world
  is the *right* expedition, not a consolation for a failed one.
  **The reason to check rather than assert it:** that argument is exactly the shape a fallback uses to
  justify itself, and this repo's rule is unusually strict ("no backup modes, no rollover behavior").
  The honest test is whether the two paths can ever *disagree about what the player is playing* — if a
  campaign can silently alternate between directed and authored worlds without the player being told,
  it is a second path however it is framed.
  *Falsify it:* delete the archive mid-campaign and check the player can tell. `pick_next_challenge`
  currently `info!`s it, which is invisible in a shipped build.
  *If wrong:* surface it in FVS-L-4's briefing — "AUTHORED UNIVERSE (no archive)" — so the state is
  legible rather than silent. That is probably the right move regardless. · *Deps:* H-3, L-4 · *Reading:* — (no corpus resource)

---

### Push 7 — SCP-9191 Antagonist & Late Roster  ·  Tier 3 / endgame  ·  M4–M5
**Goal:** the endgame — SCP-9191 as the generator whose output *is* the uncanny valley — plus the deferred "greatest-hits" roster (173/096) that needs the new per-entity watch primitive. Placed adjacent to Push 6 because **the antagonist is a generator**; its reading list overlaps the QD/generation push, not a narrative silo. The uncanny-valley papers aren't flavor — "perceptual mismatch / atypical features" is *why* generated monsters read as ugly and can drive the generator's output aesthetic.
**Reading:** **[UV-REV]**, **[UV-FMRI]**, [QD-OEE], [QD-PCG], [GOAP]
**Done when:** the endgame trigger fires after a curriculum threshold; confrontation mechanics derive from the SCP-9191 generator theme; 173/096 are capturable via a new per-entity continuous-watch state (explicitly distinct from the ambient field); no shipped copy cites the deprecated semiotic-decay theming as canon.

- **FVS-C-7 — A second gaze-reactive creature, via the ATTENTION sign-flip** · M
  `ATTENTION` already drives SCP-1048's out-watch capture (a creature *suppressed* while watched). The
  inverse — one that acts **only when not observed** — is the same channel with the condition flipped,
  so it is architecturally free: no new primitive, no new field, no append to a hashed enum.
  **Explicitly distinct from FVS-C-6.** C-6 needs a genuinely new per-entity, directional
  continuous-watch state (facing vs a *specific* entity) and is XL; this one reads the existing
  ambient field and is M. Doing this first is also the cheap way to prove the ambient/per-entity
  distinction is real before paying for C-6.
  · *Deps:* C-3 (shipped) · *Touches:* `src/ai/`, new creature module · *Reading:* [STIG], [GOAP]
- **FVS-C-6 — (LATE) 173/096 + per-entity continuous-watch** · XL · *determinism: FixedUpdate; facing math bit-exact (watch ARM↔x86 f32, J-3)*
  Add 173/096 **only after** the bespoke roster is proven. Each needs a **new** per-entity continuous-observation state (directional/facing check vs a *specific* entity), explicitly distinct from the ambient `ATTENTION` field — new engineering, not a sign-flip reuse.
  *Done when:* a per-entity `ObservedBy`/facing check drives 173/096 freeze/aggro; documented as separate from ATTENTION; capture rules authored on top. · *Deps:* C-1..C-5 shipped, E-*, F-*, M-1 · *Touches:* new watch module, `src/ai/`, `src/enemy.rs` · *Reading:* [GOAP]

---

### Push 8 — Determinism & CI Hardening  ·  cross-cutting  ·  continuous
**Goal:** protect the golden-test discipline as the surface area grows. (J-1 and J-2 already live in P1/P2, pulled early; the remaining items are the CI backbone.)
**Reading:** [TEST-OW], [TEST-NT], [ABM]
**Done when:** the deterministic core runs on ARM and x86 in CI; the harness lane gates merges; new panics/unsafe are blocked.

> **⚠️ Editing `assets/` mid-run invalidates the run, and the guard for it is now mechanical (2026-07-27).**
> `config::load_game_config` reads `config.ron` from disk every time an `App` boots, and `GameConfig` is
> `deny_unknown_fields` — so adding a slice while a suite is in flight makes every *later* test parse a
> file its binary was not compiled against. What you see is `Unexpected field named 'x' in GameConfig`
> from tests that have nothing to do with the edit. It cost a full re-measure twice in one session,
> the second time to the person who had just written the warning down — which is the argument for not
> leaving it as a warning. `load_game_config` now records the file's mtime on first load and **fails
> with the actual diagnosis** if a later load sees a different one. Treat `assets/` as frozen while a
> suite runs; the same hazard applies to `site67.ron`, the `.wgsl` files and the elite overlays.
>
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

- **FVS-J-5 — Make harness CI lane gating** · S · ⚠️ **HALF LANDED 2026-07-28 — the lane is split; the gate flip waits on one number**
  > ### The blocker was two tests, not a slow lane
  > Measured on CI 2026-07-28: with `search_parallel` already moved out, the harness lane still ran
  > **over 2 hours** (start 15:29:52, still going at 17:31) against 3–4 minutes for every other lane.
  > Local per-target breakdown from the same code:
  >
  > | target | time | share |
  > |---|---|---|
  > | **`replay.rs`** | **2050 s** | **93%** |
  > | `session.rs` | 130 s | 6% |
  > | 11 others | ~190 s | 1% |
  >
  > and inside `replay.rs`, `search_rollouts_are_reproducible_under_load` + its mutant sibling are
  > **1966 s of that 2050 s** — measured directly in a dedicated run. So **two tests out of ~90 were
  > ~89% of the entire lane.**
  >
  > *Shipped:* both moved to the nightly job, which is renamed **`search + rollout determinism
  > (nightly)`** and stays a HARD job. Same cadence argument that moved `search_parallel`: they
  > replicate 7200-tick rollouts under 8-thread load, and what they guard — rollout reproducibility
  > under contention — changes only when the search or the sim changes.
  >
  > This is the lever this entry already named ("splitting the slow replicate tests onto a nightly"),
  > now with the numbers to justify which ones.
  >
  > ⚠️ **The gate flip is deliberately NOT in this change.** Local arithmetic says the lane drops to
  > ~8 min, and CI ran ~3× local, so ~25 min — **but that is an extrapolation, and extrapolation has
  > been wrong twice today** (the mini-bake's "1 hour" was 4, and this lane's own runtime was projected
  > from a time-to-failure). Promoting a lane to a hard merge gate is a decision about how long every
  > merge waits, and it needs the measured post-split number, which the next CI run produces.
  >
  > ⚠️ **Also corrected: `Deps: H-1` never held.** The lane runs no search — `search_parallel`'s archive
  > tests were already skipped out of it — so nothing it executes depends on a baked archive.
  >
  > **The lane has already earned promotion on evidence.** It caught a real defect on PR #68 that the
  > pure-CPU gate could not see: FVS-I-1's containment constraint was scoped into the *shared*
  > `minimal_criterion` and broke `playtest_level`, which is `test-harness`-gated and therefore
  > invisible to `cargo test`. That is exactly the class of regression an advisory lane fails to stop.
  >
  > *Remaining:* one green post-split CI duration, then flip `continue-on-error: false`.
  Promote the advisory (continue-on-error) harness lane to a hard gate once retrain (H-1) stabilizes archives.
  ⚠️ **A large chunk of the runtime blocker is fixed (2026-07-27), and here is the MEASURED number.**
  `tests/replay.rs` took **3201.8 s with 19 tests** and takes **2029.8 s with 18** — so
  `zz_localize_g0` cost **1172 s ≈ 19.5 minutes**, about 28% of the lane. (An earlier note here claimed
  "53 of ~70 minutes". That was inferred from "the probe dominates" rather than measured, and it was
  wrong — recorded rather than quietly edited, because this backlog's whole discipline is that an
  unmeasured claim is worth less than no claim.) The probe was explicitly labelled *"TEMP … Remove once
  the tie-break is found"* and ran 25 full 7200-tick episodes under 8 busy-loop threads. G0 *was* found
  and fixed (`docs/rl/2026-07-16-…`), and the property it diagnosed is still asserted twice by
  `search_rollouts_are_reproducible_under_load` and its mutant sibling. A **localizer** is the right
  tool once one of those goes red, and it is 40 lines of `trace_episode` to write then; paying an hour
  per CI run to keep it warm is not. Removed. A lane nobody will wait for never gets promoted.
  ⚠️ **And removing it revealed the real cost centre: `tests/search_parallel.rs`.** It had been
  *failing fast* in every recent measurement (129 s, on an unrelated config error), so nobody had seen
  it actually run. It takes **~60 minutes** — longer than `replay.rs` — because
  `parallel_search_reproduces_the_inline_archives_bit_for_bit` runs a full inline search *and* a
  worker-process search and compares the archives bit for bit. That is a load-bearing guard (it is what
  makes `--jobs N` trustworthy, and it was red for months) so it must not simply be cut; but **J-5's
  blocker is this test, not the probe I removed**. The honest options are a nightly lane for it, or
  shrinking its search budget while keeping the comparison exact.
  `replay.rs` is **~34 minutes**, essentially all of it the remaining 18 tests, so J-5 is not free yet:
  `deterministic_core_is_bit_identical_across_many_builds` alone builds 24 `App`s, and the two
  `search_rollouts_*_under_load` tests run replicate 7200-tick rollouts. Those are load-bearing and
  should not be cut; the next honest lever is running the lane's targets in parallel *processes* (they
  are only serialised within a binary today) or splitting the slow replicate tests onto a nightly.
  ⚠️ **RE-MEASURED 2026-07-28 — the stated dep is wrong and the real gate is one number nobody has.**
  * **`Deps: H-1` does not hold.** The entry says "promote … once retrain stabilizes archives", but the
    lane does not run a search: `search_parallel`'s two archive tests are already **`--skip`ped into the
    nightly job**. Nothing this lane executes depends on a baked archive.
  * **The runtime blocker is already half-solved.** Measured on this box 2026-07-28: `search_parallel`
    **3475 s (58 min)** — correctly moved to nightly — and the rest of the lane **866 passed / 0 failed**
    with `replay.rs` at 2059 s (34 min). But that is a 24-core workstation; a GitHub runner is far
    slower, and the local figure does not transfer.
  * **What is actually missing: one GREEN harness run's CI duration.** Every observation so far is a
    time-to-*failure* (6 m 5 s, on `playtest_level` — a real bug, since fixed), which says nothing about
    the green path. A per-PR hard gate is a decision about how long a merge waits, and that number has
    to be measured, not estimated.
  **This lane has already earned promotion on evidence.** It caught a genuine defect on PR #68 that the
  pure-CPU gate could not see — FVS-I-1's containment constraint was scoped into the *shared*
  `minimal_criterion` and broke `playtest_level`, which is `test-harness`-gated and therefore invisible
  to `cargo test`. That is precisely the class of regression an advisory lane fails to stop.
  *Done when:* harness lane blocks merge on regression. · *Deps:* ~~H-1~~ — **one green CI run's
  duration**, then a decision on acceptable merge latency · *Touches:* CI · *Reading:* [TEST-NT], [TEST-OW], [ABM]

---

### Push 9 — Engine & Housekeeping  ·  cross-cutting  ·  continuous

> **All items in this push have shipped** — see `BACKLOG_ARCHIVE.md`. The goal and done-when above are kept as the record of what it was scoped to deliver.
**Goal:** the corpus-free tech-debt and one nav bug, clustered precisely *because* they don't draw on the research spine — safe to hand to whoever has spare cycles.
**Reading:** mostly none; [QD-OEE] for N-2, [ABM] for N-4, [ECS] generally
**Done when:** the large files are split without changing golden hashes, orphaned weight is gone, and the doorway nav bug is fixed.


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

  **🔧 FIXED 2026-07-27 — the soup key is now the mesh's authored PATH, not its `AssetId`.**
  `bake_autogib` sorted its vertex soup by `(AssetId<Mesh>, world-matrix bits)`. An `AssetId` is an
  **arena slot assigned by async load order and slot recycling** — the same class of value `seed_from`
  was condemned for hashing, ninety lines earlier in the same file. The comment beside it even stated
  the assumption out loud: *"the asset id is stable across same-seed runs (measured)"*. That
  measurement was taken idle, and N-8's residual only reproduces under heavy load — TESTING.md
  invariant 13 exactly, and the same mistake in the same file twice.
  `sort_total!` proved the key was **unique**, which is not the same as **stable**: a unique key drawn
  from a load-order-dependent allocator still permutes the list. Uniqueness was never the property this
  needed, and that is the generalisable lesson — *a total-order check does not check reproducibility.*
  *Shipped:* the key is now `(mesh asset path, world-matrix bits)`. glTF sub-meshes are path-backed
  (`characters/valkyrie.glb#Mesh0/Primitive0`), so it is authored rather than allocated and identical
  across runs, processes and machines. A sub-mesh with **no** path refuses the whole bake loudly rather
  than falling back to its `AssetId` for that one entry — a partial fallback would reintroduce the
  instability intermittently, which is worse than not baking.
  **Also removed:** nothing. `GibSeq`'s cross-tick accumulation is a real latent amplifier of exactly the
  shape already fixed once for the scatter seed, but the code shows **no mechanism that makes its count
  differ** between two same-seed runs, and changing it would move gib positions for no measured reason.
  Left documented rather than "fixed" on suspicion.
  *Measured, and stated precisely because the last claim here had to be retracted:* the documented
  reproducer (`cargo test >/dev/null` then the full `session` target under `--test-threads=1`) was run
  **five times**. **The gib-split assertion did not fire once** — it previously fired roughly once per
  run — and `tests/autogib_determinism.rs` stayed green throughout. That is the specific failure this
  item records, reproduced clean five times under the condition that used to break it.
  **It is not a clean sweep, and the difference matters.** Run 4 of 5 failed a *different* test
  (`a_squad_wipe_resolves_the_run_to_defeat`) on a *different* mechanism: `step_until_autogib_ready`
  gave up — "the fracture bake never completed" — which is the settle timing out, not gib state
  diverging. **That is FVS-N-11, now reproduced and diagnosed**; see its entry. The two were plausibly
  always one family of symptom and two causes, which is why N-11 was filed next to this.
  Ruled out while diagnosing: the new path-keyed lookup is not the cause of that timeout — the
  `has no asset path` refusal never fires, and four of five runs completed bakes normally.
  **Leave this entry open until it has survived several full harness runs.** If a gib split recurs, the
  next places to look are `GibSeq`'s cumulative counter and `drain_gore`'s `g.source` key (an
  `AssetId<WorldAsset>`, harmless only while all five operatives share one figurine GLB).
  **The claim to distrust is my own:** the commit that fixed the seed said N-8 was closed on the
  strength of a clean 48-minute replay and a green un-ignored reproducer. Both were true, and both were
  measured under a *lighter* load than the one that still breaks it — TESTING.md invariant 13 exactly.
  **Corrects a standing claim in Push 8:** *"adding a resource is hash-neutral ⇒ no gameplay path keys
  off entity ids"* was inferred from the victory/timer path, which never spawns a gib. Registering
  `site::SitePlugin` (one bodiless `Startup` entity) was enough to perturb load timing and turn this
  latent bug into a hard failure of `both_terminal_paths_are_bit_reproducible` — which is how it was
  finally caught. · *Touches:* `src/autogib.rs`, `src/gore.rs` · *Reading:* [TEST-NT], [ABM]

---

### Push 10 — Operative Knowledge  ·  Tier 3  ·  M3–M5  ·  **the progression system**

> **All items in this push have shipped** — see `BACKLOG_ARCHIVE.md`. The goal and done-when above are kept as the record of what it was scoped to deliver.
**Design:** `docs/2026-07-26-site-hub-and-operative-knowledge.md` §3 — read it before starting any item here.
**Goal:** operatives accumulate *beliefs* about kinds of thing, beliefs **propagate** between them and across runs, and belief changes behaviour. This **replaces squad levelling**, which would have violated F-2 ("+X%" unlocks) on self-determination grounds.
**Reading:** **[MISPERCEPT]**, [EPISTEMIC], [SDT-00], [GRIP], [ECS]
**Done when:** an operative who has met SCP-1048-A behaves differently near one than an operative who has only *heard* about it; a false belief can spread through the squad and be corrected.

**Why this is not levelling.** A level is a scalar that makes an operative better at everything, everywhere, forever. A belief is a proposition about a **kind of thing** that only acts when that kind is present: contextual, legible ("Okafor knows 1048-A is lethal"), *transmissible*, and capable of being **wrong**. None of that is true of a number going up.


---

---

### Push 11 — Render & Art Direction  ·  cross-cutting  ·  continuous

**Goal:** make the game *look* like the fiction. The renderer was doing almost nothing — an LDR camera, a normal-blind uniform ambient, no shadows, two diffuse textures for the whole dungeon — so no amount of art would have read. Fix the renderer first, then give it surfaces and content worth lighting.

**Done when:** surfaces respond to light (normal + ORM maps against an irradiance environment), the level reads as more than one place (biomes), and the asset library's untapped depth is reachable through the existing data-driven manifest rather than new code.


- **FVS-Q-8 — Biome is chosen PER CELL, so one room has carpet floor and concrete walls (REPORTED FROM PLAY 2026-07-30)** · M · ✅ **FORK DECIDED 2026-07-30 — option (b); implementation still open**
  > **Decision (user, 2026-07-30): (b) — corridors inherit one endpoint room.** Rooms sample the noise
  > at `rect.center_cell()`; a corridor takes the biome of its lower-`RegionId` endpoint. Chosen over
  > (a) because an independent corridor draw produces concrete → carpet → concrete sandwiches, which
  > read *busier* and are the opposite of the complaint; over (c) because one-biome-per-level makes
  > `biome_scale` dead config, and dead config then has to come out of `config.ron` + `world_genome` +
  > `genome_coverage` — a genome-length change bought for a cosmetic result.
  > **No config, genome or archive change**, so this does not invalidate a bake.
  > *Implementation note:* the whole change fits behind `Dungeon::biome()` (`src/dungeon/mod.rs:335`),
  > which today forwards straight to `biome_at`. Its seven callers (`dungeon/render.rs` ×4,
  > `fog.rs:217`, `audio.rs:693`, `perf_probe.rs:251`) are unaffected if the per-zone resolution
  > happens inside that one function, and `corridor_of`/`NO_CORRIDOR` (`mod.rs:146,164`) already
  > distinguishes corridor floor from room floor. Update the `biome()` doc comment while there — it
  > currently claims walls agree with their room, which is the thing that was never true.
  Player: *"I don't like backrooms carpets and concrete walls. It should be one or the other. The
  transition should be at a doorway."*
  **Cause:** `Dungeon::biome()` evaluates `biome_at(seed, cell, mix, scale)` — value noise sampled
  **per cell**. A wall cell and the floor cell it is attached to can therefore fall on opposite sides
  of the threshold *inside a single room*, which is exactly the reported symptom. The doc comment
  claims "a wall belongs to the biome of the cell it is attached to", and that holds only
  probabilistically, which is to say not at all.
  **Fix = sample per ZONE rather than per cell.** The fork, kept for the reasoning behind the pick:
  * **(a)** rooms sample the noise at `rect.center_cell()`; corridors get their own biome. Transition
    at every doorway — but a concrete → carpet corridor → concrete sandwich makes levels read *busier*,
    which is the opposite of the complaint.
  * **(b)** corridors inherit one endpoint room (lower `RegionId`). Every transition still lands at a
    doorway, corridors always match a neighbour, and `biome_scale` keeps a real meaning (how likely
    adjacent rooms differ). **Smallest diff — no config, genome or archive change.**
  * **(c)** one biome per level. Simplest to state, but it makes `biome_scale` **dead config**, which
    should then be removed from `config.ron` + `world_genome` + `genome_coverage` rather than left as
    decoration — a genome length change for a cosmetic result. Partly closes FVS-N-21.
  *Done when:* no room shows two surface treatments, and every transition is crossed at a threshold.
  · *Deps:* — · *Touches:* `src/dungeon/{mod,biome}.rs` · *Reading:* — (no corpus resource)
- **FVS-Q-7 — The flesh spread: SCP-610 as a growing field, not a standing figure** · L · *determinism: core*
  The Upside Down read — flesh growing down halls. **The engine already exists:** `src/mycelia/` is a GPU Physarum + Gray-Scott field with world-XZ floor *and* wall materials that already forages toward blood pools and nests and "blooms in the unseen dark". Missing only a flesh skin and 610 wired as a source.
  Grounded, because spread on a lattice of rooms is a solved modelling problem: **Mollison 1977** (`10.1111/j.2517-6161.1977.tb01627.x`) decomposes it into *growth* plus a *contact distribution* — exactly the split here — and warns realistic models must be nonlinear and stochastic, against a fixed-radius flood fill. **Ludlam, Gibson, Otten & Gilligan 2011** (`10.1098/rsif.2011.0506`) fit fungal spread across discrete lattice sites and show **synergy is necessary** — nearest-neighbour transmission alone cannot explain real dynamics. **Neri et al. 2011** (`10.1371/journal.pcbi.1002174`) show experimentally that **host heterogeneity lowers invasion probability**, so `dungeon.room_types` becomes a designed brake rather than decoration. Turk 1991 (`10.1145/122718.122749`) is the graphics-side classic behind the Gray-Scott layer already running.
  **The catch:** this is gameplay, so unlike the mycelia cosmetics it cannot hide behind the render-app firewall. `FixedUpdate`, seeded from the run seed, **no GPU readback in the decision path** (the existing edge in `fruit.rs` is explicitly in the same non-determinism class as physics), and a stable total key over `RegionId`. Quarantine finally becomes counter-play: cordon a room, cut that edge of the graph. · *Deps:* C-1, B-6 · *Reading:* [ABM]

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
- ~~**Determinism model is an unforced decision.**~~ ✅ **DECIDED 2026-07-27 (Director): PER-PLATFORM GOLDENS.**
  `f32` gameplay math is not guaranteed identical across instruction sets, so one hash cannot hold on
  both x86-64 and aarch64. Of the three options:
  * **A tolerance was rejected** — exact-hash discipline is what has caught *every* determinism bug this
    project has found, two of them on the day of the decision. An epsilon would blind the one oracle
    that works.
  * **Fixed-point was rejected for now** as a large invasive change to movement/fields/ORCA. It remains
    the only option that makes a replay portable *between machines*, so it is the right answer if
    cross-platform replay ever becomes a requirement.
  * **Per-platform goldens** keep each architecture held to **bit-exact** reproducibility against
    itself, which is the property every golden actually relies on.
  *Shipped:* `GOLDEN`/`GOLDEN_FIELD` are `cfg(target_arch)`-selected. aarch64 is deliberately left
  **unpinned** rather than guessed — it fails loudly, prints the hash it measured, and says to pin it
  once the `determinism-arm` lane reproduces it across builds.
  **The cost, stated plainly:** a replay or campaign captured on one architecture is **not** verifiable
  on another, and harness results cannot be compared across a heterogeneous fleet.
  **This unblocks FVS-C-6** — the standing "do not ship C-6 on divergent floats" instruction is
  satisfied by each platform having its own pin, so 173/096's facing math is no longer gated on it.
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