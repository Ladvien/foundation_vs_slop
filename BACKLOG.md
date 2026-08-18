# BACKLOG.md — Foundation vs. Slop

> **Completed items live in `BACKLOG_ARCHIVE.md`** (split out 2026-07-30). This file lists only what is
> still open. IDs are never reused, so an ID absent here has either shipped or was never issued.
>
> **The editor has its own file: `EDITOR_BACKLOG.md`** (split out 2026-08-18). `emerge-mapper` is not a game
> dependency, so nothing in it can move a determinism golden and nothing in it waits on a milestone here. Its
> IDs are `FVS-S-*`, a distinct epic from Push 12's `FVS-R-*` world-building series; an S item and an R item
> touching the same file is expected. Completed S items archive to `BACKLOG_ARCHIVE.md` under their own heading.

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
- **FVS-N-28 — The AUDIO archive collapsed to 3 of 64 cells AGAIN, reproducibly (MEASURED 2026-08-01)** · M · *determinism: offline*
  The overnight chain baked `audio` for **2 h 54 m** across 24 islands. Every island produced an elite,
  and the resulting archive occupies **3 of 64 cells**.
  **That is not a new number.** The 2026-07-28 descriptor sweep measured the audio archive at *exactly*
  `3/64` and filed it on the degeneracy watchlist. Two independent bakes, months of genome change apart
  (`audio_genome::N` grew 15 → 16 for SCP-610's drone in between), landing on the same three cells is a
  **structural** result, not sampling luck — which is the strongest evidence yet that the pairing, not
  the search, is wrong.
  **The suspect is the one FVS-I-6's audit already named and nobody has ruled on:** `audio_genome`
  (16 acoustic knobs) is binned on `swarm_descriptor` — the *swarm's* aggression × persistence. Acoustic
  knobs move what agents *hear*; the descriptor measures what the swarm *does*. Two genomes differing
  only in acoustics land in the same cell and the winner is decided by evaluation luck, which is the
  archive-collapse mechanism this repo has now hit three times (policy archive, biome genes, this).
  ⚠️ **Do not ship `elites_audio.candidate.ron`.** A 3-cell archive is not a QD archive; sampling it
  gives the director three acoustic worlds. The bake was not wasted — it is the measurement — but the
  artefact is not usable and re-baking without changing the descriptor will produce 3 cells again.
  *Done when:* the audio genome is binned on a descriptor its knobs can move, and a bake fills a
  materially larger share of the archive. · *Deps:* **I-6** (the descriptor call is yours) · *Touches:* `src/squad_ai/audio_genome.rs`, the descriptor · *Reading:* **[QD]**, [ME], [QD-PCG]
- **FVS-N-29 — The LEVELS objective is near-saturated: most elites score ~1.0 (FOUND 2026-08-01)** · S · *determinism: offline*
  The same chain baked `levels` in **83 s** (24 islands, 24/24 produced elites, **61 of 64 cells** — a
  genuinely healthy archive by coverage). But the fitness spread is the problem: **two elites at exactly
  `1.0`** and a long clump at `0.98`–`0.997`.
  A static objective most of the population maxes has no gradient left to climb, so further search
  buys diversity of *descriptor* but not of *quality*, and "best fitness" stops discriminating between
  levels. Coverage looking excellent is exactly what hides this — 61/64 reads as success.
  Worth checking before the director is tuned against these numbers, since as of FVS-H-8 the sampled
  cell now actually reaches the world.
  *Done when:* the objective either discriminates across the archive's top band, or is documented as
  deliberately saturating with the reason. · *Deps:* — · *Touches:* `src/squad_ai/level_quality.rs` · *Reading:* [QD-PCG], [ME]
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
  > ### 📐 SUPERSEDED AND SEQUENCED 2026-08-01 → `docs/2026-08-01-acoustic-program.md`
  > The Director asked the larger question — how music, ambience and SFX become gameplay worth evolving
  > — and the answer reframes this item as **stage 1 of four**, and as the prerequisite for FVS-N-28's
  > descriptor fix rather than a peer of it.
  > **Stage 1 is the noise verb, and it is the smallest of the four because the machinery already
  > exists and is unused:** `NOISE_SWARM` propagates, `crab_draw_to_din` scales the pull,
  > `investigate_threshold` gates it, and `Mode::Investigate` is already in the brain. What is missing
  > is only a way for the player to write into the channel *deliberately* — a thrown noisemaker or a
  > shot into a far wall that pulls the swarm off the extraction route.
  > Grounded in Grimshaw & Schott's affordance argument (one actor's sounds "morph other players'
  > soundscapes and so provide **new affordances**") — sound is a thing you USE, not only a thing you
  > leak. It also makes the quiet half a decision: noise discipline is only a trade-off if noise is
  > sometimes worth spending; a pure cost is a tax.
  > ⚠️ **Must not add a `Mode`** — `MODE_COUNT` sets `NeuralPolicy::WEIGHT_COUNT` and would invalidate
  > the policy archive baked 2026-08-01 *by width*. `Investigate` is the honest model anyway.
  > *Evolvable:* lure strength, dwell, and **habituation** (how fast a swarm stops answering a repeated
  > trick) — the last is what stops the verb being a solved button.
  > 📋 **Four costed options researched 2026-07-30 → `docs/noise_discipline_options.md`. Needs your pick.**
  > **A** `MOVE QUIET` latched stance (S/M — copies the `HoldFire` non-`ArmedTool` shape exactly) ·
  > **B** din meter in the HUD (S) · **C** graduated investigation, the Mafia III recognition model
  > (M/L — **moves goldens**, new per-agent `FixedUpdate` state) · **D** tier the deposit peaks by event
  > type (S).
  > *Recommended order:* **D + B** first (neither touches the pinned core), then **A** if you want the
  > verb; hold **C** until N-13 and the re-pin question are settled, since it is the only one that moves
  > goldens and would land on top of a known live leak.
  > ✅ **Half that hold is released (2026-07-31): N-13 is FIXED** (`fae5ef2`), so **C** would no longer
  > land on a live dungeon leak. What still stands is the re-pin question — C adds per-agent
  > `FixedUpdate` state and moves the goldens, which is a measure-and-re-pin either way.
  > **One finding worth keeping even if none of the options ship:** a stigmergy channel with a `deposit`
  > and an `evaporate` rate **is** Crytek's ADSR perception envelope (*Game AI Pro 1* ch. 31), except
  > spatial as well as temporal. What is missing is their *balanced peaks per event type* — footstep,
  > bolt impact, shot and death currently deposit at rates set independently rather than as a ladder.
  > And the repo's split of AI-hearing from audio playback (`sim_harness.rs:282`) is exactly what
  > *Game AI Pro 4* ch. 16 (Mafia III) argues for — **validated, do not "unify" them.**
  · *Deps:* — · *Touches:* `src/audio_tuning.rs`, `src/squad.rs`, `src/ui/` · *Reading:* **[STIG]**, [STIG-AD], [PHERO-V], + *Game AI Pro* 1 ch.31 / 4 ch.16, Grimshaw & Schott 10.26503/dl.v2007i1.313, Boonen & Mieritz 10.26503/dl.v2018i3.1051
- **FVS-N-21 — The biome genes are invisible to the level descriptor (FOUND 2026-07-30, audit)** · S
  Q-3 added `biome_mix`/`biome_scale` to `LevelGenome` because `CLAUDE.md` requires wiring features into RL/QD. **Audit says the descriptor cannot see them.** `level_quality.rs:72` has exactly two axes — `furniture_per_room / 8` and `infestation / 0.5` — and biome moves neither: furniture keys on room *tags*, mould affinity keys on room *tags*, and `score()` never reads biome. Two genomes differing only in biome land in the same cell with the same fitness, so the winner is decided by whatever else differs, or by evaluation luck — which is materially worse while N-13 is live. This is the archive-collapse mechanism that already bit the policy archive once.
  **Three ways out:** remove the genes (biome is an authored art choice, and `docs/animation.md` already establishes cosmetic-only systems as a documented exception); add a third descriptor axis (the archive is 2-D — expensive for a cosmetic dial); or **couple biome to something the descriptor already measures** — concrete resists mould, carpet harbours it — which makes `biome_mix` move the `infestation` axis with no archive change and is lore-plausible. Leaving it as-is is the one option to avoid.
  > 💡 **FVS-Q-8 made option 3 materially cheaper (2026-07-30).** Biome is now resolved **per zone**: every
  > cell of a room shares one treatment and a corridor inherits an endpoint room. So "concrete resists
  > mould, carpet harbours it" is now a clean **per-room** property — `mould affinity keys on room tags`
  > already, and biome is now the same shape as a room tag. Before Q-8 the coupling would have had to be
  > written against a per-cell mosaic, where "this room is carpeted" was not a well-formed statement.
  > It also raises the payoff: with rooms uniformly one biome, `biome_mix` shifts what fraction of *rooms*
  > harbour mould, which is exactly the `infestation` axis the descriptor already measures. Still your
  > call among the three — but option 3 is now the cheap one, not the clever one.
  · *Deps:* — · *Reading:* [QD]
- **FVS-I-5 — `containment_criterion` still gates the squad and swarm archives (FOUND 2026-07-28, review)** · M · *determinism: offline*
  FVS-I-1's constraint was moved out of the shared `minimal_criterion` and into `coevolve/search.rs` —
  but it landed inside `score_triple_compact`, whose `None` **discards the whole triple**. So a
  capture-hostile world drops the squad and swarm candidates evaluated alongside it, which is precisely
  the coupling the scoping fix existed to remove. The constraint is correct; its *placement* still is
  not. Correcting it means letting the world candidate be rejected independently of its partners, which
  changes what the co-evolution admits — so it wants a probe run, not a quick edit.
  ✅ *Re-confirmed still live 2026-07-30* — `score_triple_compact` (`coevolve/search.rs:151-172`) runs
  `containment_criterion` on both rollouts and returns `Ok(None)` for the **whole triple** on either
  failure. Its own comment says the constraint is applied "HERE, at the world archive, and nowhere
  else", which is true of where it is *evaluated* and not of what it *rejects*. · *Deps:* I-1 · *Touches:* `src/squad_ai/coevolve/search.rs`
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
  **Data point 2026-07-31, same test, same family — logged, not dismissed.**
  `session::the_wipe_paths_actors_and_fields_are_reproducible_under_load` failed **once** during a
  multi-target harness run (`session nav liveness containment squad`) that was sharing the box with a
  concurrent `cargo check --all-targets`. It then passed **2/2** on re-runs: once idle (82 s) and once
  under a deliberate 14-busy-loop load that was verifiably biting (115 s).
  Per this entry's standing instruction that is **not** an exoneration — both passes were measured
  under a *different* condition than the failure, and the specific condition that failed (a full
  24-core compile running alongside the harness) is arguably closer to the CI oversubscription this
  item is actually about than a busy-loop generator is. **A concurrent `cargo` build is worth adding
  to the reproduction recipe.**
  ⚠️ **And the divergence detail was LOST, which is my error and the reason this data point is weaker
  than the two above it.** The run piped the harness through `grep -E "^test |test result|FAILED"`, so
  the panic body — the one that prints the distinct `(snapshot, field)` hash pairs — was filtered away
  before it was ever written down. **The distinguishing question this entry exists to answer** (is the
  divergence *bimodal*, i.e. a flipped discrete decision, or a fresh hash each time, i.e. float drift?)
  **could not be asked of this occurrence.** Never filter a harness run's stderr to the summary lines;
  capture the whole thing and grep the file afterwards.
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
  whether any behaviour changes. If nothing does, simplify.
  > ### 🔎 UNPARKED 2026-07-31 — H-5 shipped, and the spike's own prediction CAME TRUE. Needs your call.
  > This entry predicted: *"If FVS-H-5 replaces `INFINITY` with a finite prior, the `Option` may collapse
  > into that prior and become ceremony."* It did. `interest()` is now
  > `learning_progress().map_or(0.0, f32::abs) + optimism(readings)`, and the two states it distinguishes
  > are **numerically identical**: a cell with `None` scores `0.0 + PRIOR/(1+n)`, and a cell measured
  > genuinely flat scores `0.0 + PRIOR/(1+n)`. The reading count already carries every bit of what the
  > `Option` was there to encode, so nothing downstream can observe the difference.
  > **The falsification is therefore satisfied and the simplification is available** — `learning_progress`
  > could return `f32` and `interest` lose its `map_or`.
  > **Two reasons it is NOT being done unilaterally.** (1) [EPISTEMIC]'s Fisher argument — ignorance is
  > not a uniform probability — is a *modelling* claim this repo deliberately mirrors in
  > `knowledge::Knowledge::of`; collapsing it here makes the two diverge, and that is a house-style
  > decision. (2) `learning_progress` is public API with its own asserted contract
  > (`an_unmeasured_cell_is_not_a_flat_one`), so the `Option` is doing documentation work even where it
  > does no arithmetic work.
  > *Your call:* simplify to `f32`, or keep the `Option` as an intentional statement about ignorance and
  > delete this spike. Either is defensible; leaving it undecided is the only bad option.
  · *Deps:* H-3, H-5 (both shipped) · *Reading:* [EPISTEMIC], **[LPM]**
---

### Push 7 — SCP-9191 Antagonist & Late Roster  ·  Tier 3 / endgame  ·  M4–M5
**Goal:** the endgame — SCP-9191 as the generator whose output *is* the uncanny valley — plus the deferred "greatest-hits" roster (173/096) that needs the new per-entity watch primitive. Placed adjacent to Push 6 because **the antagonist is a generator**; its reading list overlaps the QD/generation push, not a narrative silo. The uncanny-valley papers aren't flavor — "perceptual mismatch / atypical features" is *why* generated monsters read as ugly and can drive the generator's output aesthetic.
**Reading:** **[UV-REV]**, **[UV-FMRI]**, [QD-OEE], [QD-PCG], [GOAP]
**Done when:** the endgame trigger fires after a curriculum threshold; confrontation mechanics derive from the SCP-9191 generator theme; 173/096 are capturable via a new per-entity continuous-watch state (explicitly distinct from the ambient field); no shipped copy cites the deprecated semiotic-decay theming as canon.

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

- **FVS-Q-7 — The flesh spread: SCP-610 as a growing field, not a standing figure** · L · *determinism: core*
  The Upside Down read — flesh growing down halls. **The engine already exists:** `src/mycelia/` is a GPU Physarum + Gray-Scott field with world-XZ floor *and* wall materials that already forages toward blood pools and nests and "blooms in the unseen dark". Missing only a flesh skin and 610 wired as a source.
  Grounded, because spread on a lattice of rooms is a solved modelling problem: **Mollison 1977** (`10.1111/j.2517-6161.1977.tb01627.x`) decomposes it into *growth* plus a *contact distribution* — exactly the split here — and warns realistic models must be nonlinear and stochastic, against a fixed-radius flood fill. **Ludlam, Gibson, Otten & Gilligan 2011** (`10.1098/rsif.2011.0506`) fit fungal spread across discrete lattice sites and show **synergy is necessary** — nearest-neighbour transmission alone cannot explain real dynamics. **Neri et al. 2011** (`10.1371/journal.pcbi.1002174`) show experimentally that **host heterogeneity lowers invasion probability**, so `dungeon.room_types` becomes a designed brake rather than decoration. Turk 1991 (`10.1145/122718.122749`) is the graphics-side classic behind the Gray-Scott layer already running.
  **The catch:** this is gameplay, so unlike the mycelia cosmetics it cannot hide behind the render-app firewall. `FixedUpdate`, seeded from the run seed, **no GPU readback in the decision path** (the existing edge in `fruit.rs` is explicitly in the same non-determinism class as physics), and a stable total key over `RegionId`. Quarantine finally becomes counter-play: cordon a room, cut that edge of the graph. · *Deps:* C-1, B-6 · *Reading:* [ABM]

---

### Push 12 — World-building tools: composition-as-tile  ·  cross-cutting  ·  continuous

**Goal:** a kit whose meshes are never cell-sized becomes solvable, by making the **composition** the unit a WFC grammar works on rather than the mesh. `emerge-mapper` is where they are authored.

**Reading:** `docs/2026-08-09-compose-authoring-plan.md` (steps 1–5 and the decisions on the record), `docs/2026-08-09-composition-grammar-plan.md` (the grammar half), `docs/2026-08-09-unified-composition.md` (design), `docs/research/2026-08-09-grid-composition-corpus-check.md` (citations).

**Done when:** a room solved from an authored example is stampable, and the falsification criterion in the grammar plan §3 has been run rather than argued.

> **Where this stands.** Steps 1–4 shipped: capture on the Map, faces as bands, seating on Compose, and four authored site tiles pinned by `tests/site_tiles.rs`. The Compose stage now shows one composition at a time with two miniatures either side (`O`/`P`), each labelled in world space — FVS-R-1's gallery half and FVS-R-3, both archived 2026-08-09. Its seam-inspector half was designed out and is **FVS-R-13**. The central claim held — **no wall piece in the Site kit is cell-sized and every authored composition is**, so a group of floor-plus-wall is a tile where the 0.1 × 1.0 m wall never could be. A 3 × 3 room of them passes `adjacency::faults` with zero disagreements, and three stamped side by side make one continuous wall.
>
> **Both halves of this push's "done when" are now met, and the second one came back negative.** A room solved from an authored example is stampable — `Cmd/Ctrl+G`, FVS-R-7, archived 2026-08-10 — and the falsification criterion has been **run rather than argued**: 128 solves, zero enclosed regions, row 1 fires (FVS-R-9, archived). What the run bought is a mechanism rather than a guess about the fix — and **FVS-R-17 has since closed it** (2026-08-11, archived): the greedy collapse was replaced on the composition path by a constraint solver with an enclosure rule, and the same committed rows now read **2,048 of 2,048 solves reaching the plane** against 1 without the wish, with no row firing. The missing nine tile kinds were never the thing standing in the way; the solver's expressiveness was. Four caveats on that result live in `docs/research/2026-08-10-expressive-range.md` §9 and are not in its verdict line.

**FVS-R-22 · `Member::paint` has no writer.** The field exists, `emerge-bevy` renders it, `composition_fingerprint` folds it in — and `composition_from_set` hard-codes `paint: 0` (`editor.rs:4878`), so decal ordering is unauthorable. This is the author's *"then they may put a decal or two"* from the tile-authoring brief, and it is the last unbuilt verb of that description. Cheap once BUILD has a focused member: the focus already follows a drop. *Done when:* two decals at one spot can be ordered from the keyboard and the order survives a save.

**FVS-R-23 · Nothing checks the shipped kit any more.** `grammar.rs`'s `site_kit()` built its four-tile fixture in-test on 2026-08-11, which **corrected a rule violation** — `emerge-mapper/CLAUDE.md`: *"Tests do not read the shipped assets… a suite bound to it fails the day somebody imports a kit."* Nine solver tests had hung on four authored tiles, so clearing the project took the constraint solver's real-data coverage with it. The fixture is right; the gap it leaves is that no test now says the *shipped* kit learns and solves. That is legitimately an **asset-contract** test — the deliberate exception the rule names — and should say so in its doc comment the way `tests/site_tiles.rs` and `the_compose_tab_boots_and_sees_the_shipped_groups` did before they were deleted with the content. *Trigger:* once BUILD has authored a kit worth pinning. Not before — a contract test against an empty project asserts nothing.

**FVS-R-2 · Baked thumbnails for compositions — deliberately second.** The booth bakes per *descriptor*; photographing a composition means it must call `composition::expand`, coupling a startup asset cache to a schema that is still moving (`paint` landed 2026-08-09, `seating_divisions` with it, the corner question open). A cache keyed to an unsettled schema yields stale thumbnails that look right and are not. **Trigger, named in advance:** when the contact sheet needs scrolling or zooming to read. That N is the same number FVS-R-5's acceptance asked for, and it is now known: **nine** missing tile kinds for the kit, four authored. Thirty is not close.

**FVS-R-13 · The seam inspector, which the carousel gave up.** FVS-R-1 was two things: a gallery, and — at zero gap, tiles laid touching — a way to see an interface disagreement as a break in a wall run instead of as a fault list. The gallery shipped as a **carousel** (author's call, 2026-08-09), and a carousel never puts two compositions side by side at the same scale, so **the second half is not delivered and is not hidden inside the first**. It was the half that answered FVS-R-9's enclosure question by eye, two steps early. Two ways out, and they are not the same: a surface that stands *two chosen* compositions flush, or FVS-R-7's `agrees()` reporting seam faults as a list — which is a list, and a list is what this item existed to avoid. *Done when:* an author can see two tiles disagree without reading one.

**FVS-R-4 · Finish the Compose rebuild.** Remaining from the plan: the seat-step unit tests (a step is one rung of `grid::SnapLevel` over `policy.snap_divisor`, on all three axes — `seating_divisions` was retired 2026-08-11 because a tile's interior and the map it sits on were two spatial lattices for one act, and they did not even divide the same thing; **the centre is a legal seat**, so nudging out and back returns exactly), and the run-and-look pass — two decals at one spot with paint raised, a piece landing at the bottom of the tile, the focus reading clearly. *Done when:* an author has driven it and the frames are recorded.

**FVS-R-8 · The schema: typed split values, the occupancy test, and the tag axis.** **The "real solve" gate is met** (FVS-R-9 ran 2026-08-10; the solve converged every time but made no rooms, which **FVS-R-17 fixed on 2026-08-11** by replacing the collapse with a constraint solver — it makes them now), so this is unblocked. Originally: only after a real solve — **FVS-R-6 is decided** (2026-08-10: tags, shaped as Cooper's second layer; archived, and `docs/research/2026-08-09-composition-grammar-decisions.md` §10). Absolute/relative split values (CGA); the occupancy test **with `Shape.occ("noparent")` scoping in the same commit**; and **one field on `Composition`** for the tag axis — `composition.rs:87` carries none today, and the decision costs exactly that field, not a second prototype family. **The tag field is Sturgeon's image grid, not a renderer hint** — a layer keyed to the functional tile, so an adjacency rule over the lit axis stays expressible; the sequential solve itself waits for something that constrains it, and building it now would be building against a guess. **Do not retire the positional `Mount` variants** — furniture uses them and furniture is deliberately not gridded. This is where a golden is expected to move.

**FVS-R-25 · An injected click cannot place, and `Hovered` is why it is hard to tell.** Measured 2026-08-12 over BRP: with a brush armed from the keyboard the ghost tracks an injected cursor correctly, and no injected click places anything. Two things are tangled here and only one is a bug. **The gate**: `drive_place`, `drive_move`, `drive_clone`, `drive_removal` and the two ghosts ask `Query<&Hovered>`, which `bevy_picking` writes from the **window's** cursor — the one thing `view::sense_pointer` refuses to move for an agent. `view::over_ui`'s doc argues this is *correct* for mouse verbs because "a click is delivered by the picking backend, so the two agree by construction", and that holds for a real mouse; for an injected one the click is synthesised and the two do not agree, so the verb is judged against wherever the physical mouse happens to rest. **Replacing the gate was tried and reverted the same session**: it is a documented deliberate split, and it did not fix the placement — so the *second* thing is a separate cause, most likely the `Tap` press/release cadence against `PlaceDrag`'s press-then-release shape. *Done when:* an agent can place a piece over BRP, and the reason the old path could not is written down rather than guessed at. Do not change the `Hovered` split without settling the second cause first — reverting once already cost a session's worth of a wrong hypothesis.

**FVS-R-27 · The solver's uncertainty is not drawn, and contradiction is not a designed state.** Two of the four generate items chosen 2026-08-12, both unbuilt. **Uncertainty:** WFC's own legibility comes from rendering undecided cells as the blend of their remaining patterns — Karth & Smith 2017 (`10.1145/3102071.3110566`) quote Gumin on why it is *"so enjoyable to watch"*, and ecological interface design says the same thing as a rule: *"the perceptual cues (signs) in the interface should directly specify process constraints"* (Vicente & Rasmussen 1992, `10.1109/21.156574`). Today the solve is atomic and the author sees only the answer. **Contradiction:** Dajkhosh 2024 measured *"only one in every ten tries would result in an output"* with restart-on-failure and recommends backtracking instead; Cooper 2022's Sturgeon (`10.1609/aiide.v18i1.21944`) exposes **infill, link and repair** as first-class verbs. `emerge-mapper` has one verb — generate the lot — so a region that will not solve has no local fix. *Blocked on nothing but size:* both need `emerge_core`'s solver to expose intermediate state and a bounded region, which is a real API change and where a golden may move. *Done when:* an author can watch a region resolve, and can re-roll or repair one part of a map without touching the rest.

**FVS-R-20 · The geometric box is not the visual one, and snapping uses the wrong one.** **Less urgent since 2026-08-11:** the case that motivated it was a wall placed loose on the Map, and a wall is now a member inside a tile — `build::drop_at` reaches flush at -0.45 from the cell-corner rule, with no snap box and no free placement. It still bites any piece placed loose whose art is inset from its box. `brush_span` snaps by `extent.footprint`, the measured box. StickyLines names this exactly: *"Alignment and distribution commands use the geometric center of objects, but sometimes this does not match the object's visual center… **All were forced to fine-tune the result**… To our knowledge, current tools completely ignore such tasks"* (`10.1145/2984511.2984577`). **This repo already met it and wrote it down without naming it** — `policy.rs` on seating: *"`site/wall` is 0.1 m thick and sits flush at −0.45, which is not a multiple of 0.125 either, because art is authored to look right rather than to tile."* So a piece whose art does not fill its box snaps arithmetically right and visually wrong, and the only recourse is Alt free-placement, which discards the lattice entirely. **The fix is a snap box, not a snap rule:** an authored per-descriptor offset/extent used for snapping only, beside `align.pivot` and `align.y_offset`, which are already "how this art sits" fields — the cheaper, kit-shaped half of StickyLines' *tweak the bounding box… without affecting the object itself*. *Done when:* a piece whose art is inset from its box lands where it looks right, with no free placement. · *Reading:* `docs/research/2026-08-10-snapping-corpus-vetting.md` §3

**FVS-R-16 · A VLM in the loop — as the author of the metric, not the scorer of candidates.** Asked 2026-08-10: *"can we get the VLM to score these and just provide a feedback loop to the WFC grammar?"* Yes, and the corpus names the shape. **PCGRLLM** (Baek, Earle, Togelius et al. 2025, `10.48550/arXiv.2502.10906`) puts the model *"not as generators of content but as designers of reward functions … scoring, proposing edits, and reweighting terms rather than serving as the generator itself,"* and says the separation is what *"preserves low-latency inference, mitigates direct LLM biases during sampling, and enhances stability and reproducibility."* It names the vision extension directly: *"when explicit symbolic metrics are difficult to design, vision-based models can provide feedback directly from rendered outputs."* So the loop is **render solves → the model proposes or reweights a scoring function → that function runs deterministically over thousands of solves → the result feeds the next refinement**, at human cadence, never per candidate. **Three constraints make that the only version that works here:** determinism (a VLM cannot touch `FixedUpdate` or anything `snapshot_hash` sees), §4 of the decisions doc (a model tuning weights against output it has seen *is* picking a number after the fact — a model authoring a metric that is then committed and measured against pre-registered thresholds is not), and the commit door (`vlm.rs`'s pattern: prompt built from code, validated at the door, refused whole, staged for a human — a scoring proposal lands in the same holding pen as a token proposal). Most parts exist: `map_elites` is the QD kernel, `bevy_devshot` renders, `vlm.rs` is the plumbing. **Deliberately not scheduled yet, and the reason is measured:** the generator has four tiles and three adjacency pairs, so a feedback loop would be optimising the relative weight of four things plus `Empty`. The plans are not bad because the scoring is wrong — the vocabulary cannot express a room, and FVS-R-5 counted **nine missing tile kinds**. *Trigger:* when the alphabet is large enough that a human cannot hold the trade-offs in their head. · *Reading:* `10.48550/arXiv.2502.10906`, `10.1109/tpami.2024.3398998` (evaluation-metric survey), [QD-PCG]

**FVS-R-10 · `site/wall_doorway` can never be a tile.** 0.46 × 2.06 m against `CELL_EPSILON` 1e-4 — it fits neither a 1 m cell nor a 2 m one, and `grammar::learn` refuses it by name. `site/tile_doorway_n` routes around it with `wall_header` as a lifted lintel. *Done when:* the mesh is re-authored to cell size or the descriptor is retired, so nothing reaches for a piece the grid cannot hold.

**FVS-R-11 · The corner question.** Whether an interface token lives on an edge or a corner is a schema decision and hard to revisit. The reading is Lagae & Dutré, *An alternative for Wang tiles: **colored edges versus colored corners*** (`10.1145/1183287.1183296`) — the subtitle is the decision — for **layered vs replacement**: Merrell layers, and *"square tiles with colored corners"* reads like substitution. **Ingested 2026-08-10** from the authors' own copy at `graphics.cs.kuleuven.be/publications/LD06AWTCECC/`, converted and indexed; the backlog's claim that a PDF was staged at `/mnt/home-still/papers/LD/LD06AWTCECC.pdf` was false — that directory held an unrelated paper. **Nothing is blocked on it and never was:** the decisions doc §2 says the validity-function seam *"stops [the corner decision] being a prerequisite for anything"*, and FVS-R-7 shipped that seam. Read it when the schema question is actually being taken.

> **A second thread opened and closed 2026-08-16: separating the project from the kit.** `policy::layered_library` read exactly one directory, so a kit directory held a library, a policy, a tile set **and** every map — the word "kit" was doing the work of *project*. The measurement that decided it: `site/` and `site_greybox/` define the **identical 45 ids**, so a namespace is an *interface* and a directory is a *skin*, and "what does `site/floor` mean" is a question about the project. **All six stages shipped** and are in the archive — FVS-R-35 (the delete guard, which had already cost the shipped kit), -37 (a `Fixture` that can hold two kits), -36 (`Project::namespace`), -38 (the lattice settings onto the map), -40 (the solver-budget readout) and -41 (binding, the composition collection, and maps out of the kit). Design and reasoning: `docs/2026-08-16-collections.md`. **FVS-R-39 is what is left, and it is authoring rather than engineering.**

> **A third thread opened 2026-08-16: one door per thing.** Reported at the keyboard — *"When I enter a kit, I'm still getting tiles and maps and everything… there's a third entity that needs its own entry in the main UI, which is tiles."* The editor is one binary with one door and five tabs, so every door shows all five. The design splits it into **five doors — Kits, Tiles, Compose, Maps, Rigs — one per tab**, which means the tab strip stops existing and `Mode` stops being a mode. Decisions taken before the doc: Compose is its own fourth door, Rigs its own fifth, design before build. Design and measurements: `docs/2026-08-16-doors.md`. **FVS-R-42 and -43 are worth doing whether or not the doors are built** — the first is a correction the measurement forces, the second is a cut that is nearly free.

**FVS-R-42 ✅ SHIPPED 2026-08-16 · The lattice settings were one level too high.** `face_bands` and `snap_divisor` moved from per-kit `Policy` onto `Map` on 2026-08-16 (FVS-R-38) — right in direction, one stop too far. The argument then was that two bound kits disagreeing has no local answer; two **maps** disagreeing has none either. `composition::interface(comp, comps, library, per_tile)` (`composition.rs:1475`) takes the band count as an argument and every call site passes `project.map.face_bands`, so **two maps in one project at different band counts give the same tile two different adjacency contracts** — a kit of tiles is only coherent at one band count. Nothing stores a wrong answer, so this is not corruption; it is that "what does this tile present?" has no answer until you name a map, and the Tiles door is exactly where you need one with no map open. `snap_divisor` is the same shape: a tile seated on thirds, opened under a map at divisor 2, has every nudge verb move it off its authored rungs. **Fix:** a `Lattice { face_bands, snap_divisor, cell_height }` on `kits.ron` — the level at which neither two kits nor two maps can disagree. `cell_height` also retires `build.rs`'s three reads of `map.bounds.1` as a blank tile's height, which is a map fact standing in for a kit one. Touches `MAP_VERSION` → 5, the 10 non-`editor.rs` read sites, and the **game's** loader at `emerge-bevy/src/lib.rs:114`. *Done when:* a tile's interface is derivable with no map open, and `cargo test --workspace` is green. · *Reading:* `docs/2026-08-16-doors.md` §3.1

**FVS-R-43 ✅ SHIPPED 2026-08-16 · `Project` was map-shaped, and four of the five doors have no map.** `Project` carries `map`, `map_path`, `dirty` and `touched` beside the library, the vocabulary and the kits, and **113 parameter positions across eleven files** take `Res<Project>` — which makes the split look enormous. It is not, and the measurement is the whole finding: of ~270 genuine map-field accesses, **234 are in `editor.rs`, the module that *is* the Map door**, and `dirty = true` is written in that one file and nowhere else. Outside it the non-Map doors read the map for exactly four things, three of which are FVS-R-42's lattice. **Fix:** cut `OpenMap { map, map_path, dirty }` off `Project`; the Maps door inserts both, the other four insert only `Project`. Mechanical, one module, no behaviour change — worth doing on its own even if the doors never ship, because it is what makes them affordable later. *Done when:* `Project` names no map and `emerge-mapper` still passes headless. · *Reading:* `docs/2026-08-16-doors.md` §3

**FVS-R-44 ✅ SHIPPED 2026-08-16 · Doors, and the tab strip becomes the door's own.** Shipped as **five** first; the build's own measurement cut it to **Kit / Map / Rigs** (see FVS-R-45). The strip survives, scoped to `Door::tabs` — the Kit door holds Meshes/Tiles/Compose and never the Map. `Mode::ALL` becomes `Door`, `--door` joins the command line, and each door builds with one tab — so `Action::{NextTab, MapTab, MeshesTab, TilesTab, ComposeTab, AnimTab}` (`keys.rs:154`) and their six `Context::Global` bindings go. A Global row costs **every** context against the twelve-row cap `no_context_carries_more_than_a_learnable_vocabulary` enforces (`keys.rs:2673`), and `rows()` collapses the five digits to one row and `Tab` to another: the split **hands two rows back to all five contexts**. Then the menu becomes a door strip over one list and one info panel, generalising the two coupled columns — a coupling already stale since maps left the kit directories. **Three things this design decides rather than defers, all overrulable:** the Tiles/Compose boundary is Bounded-vs-Anchored envelope (which answers `2026-08-16-collections.md` §9 by construction); `emerge-mapper . site_67`'s second positional argument retires, because under five doors a bare positional cannot say which door it names; and the Kits door's mesh-pack fold (`tiles.rs:3473`) stops keying on the open map and keys on the library instead. **The risk is named and charged:** three doors to label a mesh, build a tile from it and place it is three menu round-trips, against Lai, Latham & Leymarie's second pillar — *"it is important that this feedback loop is as short as possible"* (`10.1145/3402942.3402946`). Mitigated by a door key, not by a cross-door hop, which would be the tab strip in a hat. *Done when:* entering a kit shows meshes and nothing else. · *Reading:* `docs/2026-08-16-doors.md` §§2, 4, 5, 7

**FVS-R-45 ✅ ANSWERED 2026-08-16 · Four shipped guides crossed doors mid-flow, and one crossing lost state.** Found by building FVS-R-44 rather than by arguing about it: `room_from_nothing.json` goes Meshes → Tiles → Compose → Map → Compose → Map → Compose (**seven crossings**) and `build_a_room.json` does six; `author_the_site_kit.json` and `branch_verbs.json` cross once each. Under the doors every crossing is a menu trip **and a process boundary**. **The Compose→Map pair is the sharp one:** Compose arms a tile and the Map stamps what is armed, so they are two halves of one act — and `ComposeState::armed` is a session resource, so the boundary does not merely lengthen the loop, it *loses the state the second half needs*. `build_a_room` crosses it four times. **Three answers, none picked** (this is a design decision, not a defect): (a) Compose and Map are one door — they share a subject and the arming state, which contradicts the 2026-08-16 "Compose gets its own door" decision, taken before these numbers existed; (b) arming moves from `ComposeState` into the project so it survives a door change — cheap, makes the crossing survivable rather than free; (c) the doors are **Kit** (Meshes + Tiles + Compose) and **Map**, which is close to what was actually asked for. **(c) was chosen at the keyboard** — *"Kit and Map, two doors"* — and the three tests that noticed pass again. With Compose inside the Kit door the arming state never crosses a process boundary in any shipped guide, so (b) was not needed; it becomes necessary only if a workflow later has to arm on the Kit door and stamp on the Map. **Rigs stayed its own door**, by the earlier decision and because it crosses with neither. · *Reading:* `docs/2026-08-16-doors.md` §10

**FVS-R-39 · The `site/*` namespace has no provider, and the game cannot boot.** `assets/emerge/site/`, `site_greybox/` and `site_v2/` were deleted 2026-08-16 and stay deleted by decision. `SitePlugin::build` (`src/site/mod.rs:153`) panics without a kit, so `cargo test --workspace` is red: **51 test functions** across `src/site/{kit,layout,pieces,people,smart}.rs`, `tests/site_descriptors.rs`, `tests/site_editor.rs`, `tests/mesh_measurement.rs` and `tests/importer_against_real_meshes.rs`, plus every harness test that builds the sim app (`SitePlugin` is in `src/sim_harness.rs:448`), plus three `emerge-core` examples that hardcode the path. **Two ways out and they are not the same size:** re-author `site/*` against the ozea meshes — what `site_v2` was created for, and what the Meshes tab and the VLM batch exist to make tractable — or park the Site hub, which puts `src/site/` and its 51 tests behind a decision rather than a missing file. **The author chose to re-author** (2026-08-16), and the checklist for it shipped the same day: `site::kit::tests::the_site_kit_names_every_piece_it_still_owes` reads `assets/site/kit_ozea.ron` and names **all 45 owed pieces at once**, sorted, with the architecture constraints in the message — because `SiteKit::resolve` refuses on the *first* unknown id, so re-authoring used to mean learning the next thing owed one `cargo test` at a time. Make the directory with `N` on the chooser's KITS column; nothing needs to create it by hand. *Done when:* that list is empty and `cargo test --workspace` is green. **Nothing else here is a real gate; this one is.**

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

- **⚠️ Mirrors are stale while `crates/bevy_autogib/` is untracked.** `scripts/mirror_crates.sh` refuses on a dirty tree — correct behaviour, refusing beats forcing past uncommitted work — but it is an **unowned** blocker: it clears only when that crate is committed. Check rather than assume. When it lands it also needs adding to `CRATES` in that script, plus the `README.md` (with the "Vibe Coded" banner), `CLAUDE.md`, both licenses, 1–3 `examples/`, and a `tests/leaf.rs` ratchet — the script refuses a crate missing any of them.
- **⚠️ `10.1145_1814256.1814260` is catalogued as Smith & Whitehead, *Analyzing the Expressive Range of a Level Generator*, but its indexed text is UCSC website boilerplate** — the conversion captured a landing page, not the PDF. **Do not quote it.** Cite `pcgbook-ch12-evaluating-content-generators` until it is re-fetched and re-converted.
- **A comma cannot be a key chord in `emerge-mapper`.** `keys::rows()` joins a collapsed row's chords with `", "`, so a comma chord is unreadable the moment it shares a row with anything — including its own pair. Tried for turn, caught by `collapsing_rows_loses_nothing`; tried again for paint order, caught again. It is a property of the census, not of any one row.

- ~~**Top design risk — kill-vs-capture fitness conflict (I-1).**~~ ✅ **RESOLVED 2026-07-27; this entry
  was stale until 2026-07-31.** It read *"Unresolved … a weighting/decomposition decision is required"*
  while `BACKLOG_ARCHIVE.md`'s FVS-I-1 recorded the design **and both code steps** landed four days
  earlier. Corrected during the 2026-07-31 backlog review, which is itself the lesson this file keeps
  re-learning: **a status marker is a claim someone made on a particular day.** The consequence was not
  cosmetic — two items (**H-1**'s bake and **I-5**) were reading as gated behind I-1 and are in fact
  unblocked, H-1's own "I-1 must land BEFORE this" being satisfied.
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
- ~~**Asset conversion blocks the whole Site (N-10).**~~ **Retired 2026-07-27 — this risk never materialised and was already refuted in §3.** The Site shipped greyboxed from the 145 in-repo Kenney `.glb`. N-10 remained an art upgrade and blocked nothing. **Corrected 2026-08-01:** the shipped kit is now Ozea (`assets/site/kit_ozea.ron`), so the second clause of this note — "never touched the Ozea library" — is out of date. The greybox kit is retained as `GREYBOX_KIT_PATH`, the fixture that proves the swap, and both kits are validated by every kit test. The upgrade landing did not resurrect the risk: it was authoring two RON files.
- **⚠️ TOP PROCESS RISK — "pure library, green tests, no caller" (found 2026-07-27).** Three subsystems shipped this way in one session: Push 4's research economy (FVS-E-5), the O5 economy (FVS-P-3), and operative beliefs (FVS-O-1b). All three are correct, well-grounded and fully unit-tested; none is reachable in play. FVS-B-8 caught the identical failure one push earlier and it still recurred, so treat it as **structural, not careless**: this repo's house style front-loads pure, harness-free logic — which is right, and is why the determinism story works — but green unit tests on a pure function then read as completeness. **The cheap counter is an acceptance test that drives the real `App`**, one per item, written *before* the item is called done. Every "Done when" in this backlog is phrased as player-observable behaviour for exactly this reason; the failure was not checking it.
- ~~**The ASYNC aperture's SHADER has never been looked at.**~~ **Looked at on a GPU 2026-08-02 (G-5).** Two faults, and neither was in the corridor maths — that part was sound. (1) The core term was multiplied by `charge` alone, so an aperture nobody was standing in contributed *nothing* and the door rendered as a flat sheet of sodium: the game's signature image was the dullest surface in the hub. It now carries a resting breath (~7 s). (2) **Nothing in the shader ever exceeded 1.0**, so despite the camera carrying `Hdr` + `Bloom` the aperture could not bloom at all — it was HDR-capable and entirely inside LDR. The core now goes above 1, which is what makes it read as a hole emitting light rather than a painted panel. Separately, a custom `fragment` illuminates nothing around it, so the portal used to leave the hall floor in front of it black; `aperture::ApertureGlow` is a real `PointLight` on the same breath and charge, shadow-casting so the jambs crop the spill. Still open: `depth`/`warp` remain the blind-tuned values, and the shader has no aspect uniform.

  **Its geometry has now been, and was worse than the shader (fixed 2026-08-01).** Four faults were stacked: the frame was authored `yaw: 0.0` while every wall in the `z=1` run it fills is `yaw: 90.0`, so it stood *across* the run; it was placed at the trigger's position, a metre out on the hall floor; the perimeter held four cells (4 m) clear for a 2.003 m frame, leaving a metre open either side; and the portal quad was sized from `trigger_half_extents` — a gameplay volume — giving 3.2 × 2.0 against a **measured 1.600 × 1.626** clear opening, in a plane 90° off the frame's for any yaw. The material is `AlphaMode::Opaque` by design, so that overhang punched through the wall rather than fading. A fifth was found by screenshotting the fix: the frame reaches `DOORWAY_HEIGHT` 2.0 and the walls beside it `WALL_HEIGHT` 2.4, so a **0.40 × 2.00 m slot ran straight through the perimeter** above the lintel — even though `DOORWAY_HEIGHT`'s own doc says "the wall runs continuous above it" and the dungeon honours it. `assets/ozea/wall_header.glb` (`SM_Wall_1x2` cropped to its top 0.40 m) is now placed as a real header course. The clear opening is now an art fact in the kit (`kit::DoorPiece::opening`, measured from the mesh's POSITION accessors) and `SiteLayout::validate` refuses a gap that does not match the frame. Note the shader remaps `mesh.uv` to `[-1,1]` and marches on `uv.x` with no aspect uniform, so it was being stretched 1.6:1 by the old quad and is now very nearly square — G-5's tuning starts from a much saner image. A sixth fault turned up on a full visual tour and is also fixed: the quad was **single-sided**, so from the Q/E detents that look at the Site from outside the door was a see-through hole in the exterior wall (`Material` has no `cull_mode` hook — it is cleared in `specialize`).

- **Two things the Site still has no content for (found on the 2026-08-01 visual tour).** Neither is a bug; both are missing authoring. **The research wing is an empty floor** — 12×10 cells with one wing decal and nothing else, while requisition has crates and records has a desk. **Containment cell fronts are free-standing glazed panes** with no enclosure: `site67.ron`'s `cells:` authors a `WallWindow` and nothing around it, so until a specimen is held the wing reads as six sheets of glass standing on an open deck. The second is arguably the more valuable to fix — the containment wing is the payoff for capturing anything, and FVS-D-4's "what you walk past is a rack of what you brought home" only lands if a cell looks like a cell when it is empty.
- **Blind pattern-replaces break this codebase specifically.** One warning was "fixed" by a repo-wide rename that also hit a test genuinely using the binding it renamed: one warning became four compile errors. The tests here deliberately reuse production names to pin contracts, so a name is rarely as unique as it looks. Read the site, then edit it.
- **⚠️ The replay suite cannot currently finish: `deterministic_core_is_bit_identical_across_many_builds` aborts on a stack overflow (found 2026-08-01).** `thread 'IO Task Pool (0)' has overflowed its stack` → `SIGABRT`. Bevy's async **asset-loading** pool takes a smaller default stack than the main thread, and this test builds many `App`s in one process, so each one re-enters the loader. Confirmed **pre-existing**: reproduced at `82e4f5a` in a clean worktree, i.e. before the Site/Ozea work landed, so it is not fallout from the kit swap. Not OOM — 18 GB free, nothing in `dmesg`.

  Why it matters more than one red test: `cargo test` fail-fasts across test binaries, so this abort **stops the whole harness run at `tests/replay.rs`** and every suite after it silently never executes. Any claim of "the harness is green" made without reading the per-suite list is unfounded. Likely fixes: raise the pool's stack (`TaskPoolThreadAssignmentPolicy` / `stack_size`), or drop the app between builds so the loader unwinds. Until then, run the harness with `--no-fail-fast` or nothing downstream of `replay` is being checked.

  **Correction, measured 2026-08-05.** `RUST_MIN_STACK=33554432` — which `ci.yml` sets in `env:` and a local shell does not — makes this test **pass**, so the workaround is one variable, not `--no-fail-fast`. And the claim above that *"`field_passes_are_bit_identical` passes at HEAD in isolation"* is **wrong**: it fails in isolation on Apple Silicon, because *"no field golden is pinned for this architecture yet (goldens are PER-PLATFORM)"*. Same for `migrated_defaults_reproduce_the_shipped_golden_hash`. Both would pass in CI's x86_64 lane. Measured hashes for whoever pins the `aarch64` arm: field `0xe090401cb48e2ae3`, migrated-defaults `0xac8196c4a1bfb0d0` — but see each test's own message, which asks for the `determinism-arm` lane to reproduce them across builds first.
- **⚠️ Two harness oracles are red, pre-existing, and nothing gates on them (measured 2026-08-05).** Confirmed by running `tests/replay.rs` from a worktree at `aea728b` and getting the identical result, so neither belongs to the `feat/emerge-lattice` work. The harness lane is `continue-on-error`, which is how both stayed unnoticed.

  **`photophobia_pulls_crabs_into_shadow` is fixed** (2026-08-05) — and the mechanism was never wrong. Diagnosed by measuring the A/B across five seeds and five horizons rather than reading the sign chain: the effect is real and large (`5/5` seeds at 30 ticks, pooled `0.772 → 0.568`; `−37%` pooled at 120 ticks) and by 360 ticks it is gone (`1/5`, pooled inverted). The oracle measured one seed at 360 ticks, so it was comparing two decorrelated worlds. It now pools crabs across five seeds at 120 ticks, with the shipped seed deliberately still in the set. The gradient convention was verified directly (step one tile along `LightField::gradient` and resample: brighter).

- **DESIGN CALL: should the photophobic push cross surface patches?** Surfaced by the diagnosis above and **not** decided. `crab_locomotion` runs `light_push` through `clamp_to_patch` on purpose — *"gate crossings stay with the mode's flow-field"* — so photophobia is a **within-patch** effect: a crab settles at the darkest point of its own patch, generally mid-gradient. Measured at tick 360: with the gain on, 13–19 of 40 crabs are still standing on a light gradient; with it off, 26–38 of 40 have random-walked into flat *deep dark*. So over a long horizon **diffusion into other rooms beats steering within one**, which is why the old oracle inverted. Two readings, both defensible: (a) correct as-is — light is a local force and room-scale routing is the mode's job, so "dark = cover" holds at the scale a player reads; (b) a photophobic crab should be able to leave a lit room, which needs the push to influence patch selection rather than only position within a patch. (b) changes crab distribution and therefore `snapshot_hash`, so it needs the replay gates and a golden re-pin. Do not "fix" this without picking one.

  **`authored_world_config_override_is_a_noop` was never a real failure** — corrected 2026-08-05. `GOLDEN` is `0` under `cfg(not(target_arch = "x86_64"))`, so on Apple Silicon it compared against zero; the message about a lossy seam is the assert's text, not a diagnosis. The authored config reproduces the shipped hash **exactly** (`0xac8196c4a1bfb0d0` both ways), so that seam is lossless and the QD archives riding it were never at risk. Both `aarch64` goldens are now pinned and the whole replay suite is green.

- **⚠️ Three x86_64 replay goldens are STALE, and that is what blocks promoting the harness lane (found 2026-08-05).** `migrated_defaults_reproduce_the_shipped_golden_hash`, `field_passes_are_bit_identical`, and `authored_world_config_override_is_a_noop` (which reads the same `GOLDEN`) all fail on CI's x86_64 runner. **They pass on aarch64**, whose goldens were measured and pinned the same day.

  **Why nobody knew.** On `main` this lane fail-fasted at the **lib** target on the SIGMA canary — `cargo test` stops at the first failing *binary*, so `tests/replay.rs` and every suite after it never executed. Run `30909881477` (main, 2026-08-04): `1023 passed; 1 failed`, job over. The lane was also `continue-on-error`, so the red never blocked anything. Two independent concealments stacked on the same file.

  **What has to happen, in order:** (1) measure the three hashes on x86_64 — this **cannot** be done from an Apple Silicon machine, the goldens are per-platform by design (see `tests/replay.rs`'s `GOLDEN` doc); (2) establish whether the drift is a real gameplay change or an unrecorded re-pin — `a0_fvs_j6_mutant3_on_world_0x5c09191_reproduces` and `deterministic_core_is_bit_identical` both **pass** on x86_64, so whatever moved did not move everything, which is a strong clue; (3) fix or re-pin; (4) then drop `continue-on-error`.

  **Do not promote the lane by skipping these three.** They are the determinism pins. A gate that is green because it stopped checking them is worse than an advisory lane that is honestly red.

- **⚠️ The harness lane's four known-red skips — the debt list that made promoting it possible (2026-08-05).** The lane is now a **hard gate** with `--no-fail-fast`, which took enumerating every failure at once; `cargo test` stops at the first failing *binary*, so each red hid the ones after it and reproducing the full list took one run per defect. All four are **pre-existing** — confirmed against a worktree at `main`, and the branch touches nothing under `src/squad_ai`/`src/ai`. **Each skip in `ci.yml` must be deleted the moment its test is green.**

  1. `containment::watching_the_feed_makes_it_generate_and_ignoring_it_stops` — the ATTENTION gate; fully measured, needs a placement decision. See the entry below.
  2. `playtest_level::shipped_level_playtests_and_is_deterministic` — **bisected 2026-08-05.** The static pre-filter passes (axes `(0.849, 0.313)`) and `decode` passes; the third gate is the one that rejects: `surprise::minimal_criterion` returns *"no crab died — the world was static"*. The rollout outcome on the shipped level at seed `0x5C09191`, 1800 ticks:

     `squad=5  survivors=5  crabs_alive=41  crabs_killed=0  duty_decisions=138  unit_damage=0.000  reachable=3577  liveness_violations=0`

  3. `search_calibration::a_candidate_genome_actually_changes_the_simulation`
  4. ~~`search_calibration::the_authored_brains_produce_a_real_encounter_on_every_world`~~ — **FIXED 2026-08-07**, skip deleted. Both were recorded as pre-existing when they were found during the crab/SCP-150 combat-feel work (world `0xA11CE`, Engineer brain).

  **✅ RESOLVED 2026-08-07 — the criterion half.** `minimal_criterion` now accepts a **completed containment** as a resolved encounter alongside a kill (`surprise.rs`; `captures_completed` was already on the outcome). This is the recalibration `search_calibration`'s own failure message asked for.

  Measured first, on the authored brains at 7200 ticks — a capture completes on **every** held-in world, so the gate really was rejecting the shipped game:

  | seed | captures_completed | crabs_killed | unit_damage |
  |---|---|---|---|
  | `0x5C09191` | 1 | 0 | **0.0** |
  | `0x1CE5` | 2 | 0 | 48.7 |
  | `0xFEED` | 1 | 0 | 507.5 |

  Note `0x5C09191` takes **zero damage**, so the "nothing was at stake" clause had to recognise containment too, not just the kill clause — a capture-only fix to one clause would have left that world rejected.

  **It does not weaken the degenerate filter.** A capture is thrown by the synthetic player, so like `crabs_killed` it is environment evidence; agency is `squad_duty_decisions`, untouched. New unit tests pin both directions, including that an always-carried brain with a capture is still rejected.

  **Cheaper than this entry predicted:** the criterion is applied to the outcome *after* the rollout, so unlike the other two candidates it cannot move `snapshot_hash` or the field golden — verified, both unchanged.

  **⬜ STILL OPEN — the other half: the brain barely runs.** `a_candidate_genome_actually_changes_the_simulation` and `playtest_level` remain red for the *ordered/weapons-tight fraction*, not the gate: only the ENGAGE window exercises the brain (`DWELL_ADVANCES × ADVANCE_TICKS + ENGAGE_TICKS` = 1200 ticks per hub cycle, 25% of it brain-controlled), so two genomes produce a byte-identical hash, and `playtest_level`'s 1800-tick horizon barely reaches one engage window. `tests/skip_debt.rs::the_brain_barely_runs_so_two_skips_are_still_needed` now guards exactly that ratio. Candidates unchanged: shorten `ADVANCE_TICKS`/`DWELL_ADVANCES`, or stop holding weapons tight for the whole engage window. **Both move `snapshot_hash` and re-score every archive.**

  **LOCATED (2026-08-05). One cause, three of the four skips, and it is the evaluation harness — not the game.** The decisive experiment: take the **identical** `SimConfig` the failing test rolls out (`deterministic_core_seeded(0x5C09191)` + `BrainSource::Authored` + `.with_level(pheno)`) and step it **by hand** instead of through `run_episode`. Result: crabs 40 → 44, lowest crab health **0.303**, 4 nests present at tick 1. **Combat works.** The same config through `run_episode` gives `crabs_killed = 0, unit_damage = 0.00`. The only difference is `run_episode`'s **synthetic player**.

  `search_calibration`'s own failure dump names the mechanism:

  ```
  ordered_ticks: 4500  of EPISODE_TICKS 7200   (62.5% under standing player order)
  weapons_tight_ticks: 900                     (12.5% holding fire)
  captures_attempted: 3, completed: 1, broken: 2
  crabs_killed: 0   unit_damage_taken: 0.0
  cells_covered: 332  of reachable_cells: 3577  (9% of the map)
  squad_duty_decisions: 440
  ```

  And `search_calibration.rs:60` states the consequence of the first number: *"a standing `MoveOrder` overrides locomotion and excludes the unit from `unit_actions` and `medic_heal`, so a permanently-ordered squad evaluates nothing."* The squad is ordered for nearly two thirds of the episode and weapons-tight for an eighth of it — and `WeaponsTight` gates the bolt. It is also *correct* that it does: holding fire is a containment verb, and 3 captures were attempted. **The containment mechanic and `minimal_criterion`'s "a crab must have died" are in direct conflict**, and the harness satisfies the former.

  This is why all three fail:
  * `playtest_level` and `the_authored_brains_produce_a_real_encounter_on_every_world` — `minimal_criterion` rejects on "no crab died", so **every** episode is rejected. The test's own message spells out the stakes: *"`train evolve` will silently produce an empty archive and exit 0."*
  * `a_candidate_genome_actually_changes_the_simulation` — two genomes produce a byte-identical hash, which follows: if the squad is ordered or weapons-tight for 75% of the episode, the brain barely runs and cannot differentiate.

  **The fix is a judgement call, not a bug fix, and the test already frames it:** *"Either gameplay changed, or a threshold in `surprise::minimal_criterion` needs recalibrating against `train probe`."* Gameplay did change — containment landed and holding fire became a core verb. Candidates: recalibrate `minimal_criterion` so a *capture* counts as a real encounter alongside a kill (`captures_completed: 1` is right there in the outcome); shorten `ADVANCE_TICKS` (300) so less of the episode is spent ordered; or have the synthetic player not hold weapons tight. Each moves `snapshot_hash` and re-scores every archive.

  **Do not trust my earlier eliminations for this.** Nine hypotheses were tested and eight were wrong, and the first seven measured the **shipped dungeon stepped by hand** — a world where combat works fine — rather than the harness path that actually fails. Recorded so nobody re-runs them: squad never reaches the swarm (closes to 0.47 m), crab overhead (all crabs at `y = 0.12`), horizon too short (5400 ticks, still zero), LOS blocked (`true` at the closest pair), the brain never picks a fight (no attack mode on *either* seed), the fog gate (163 visible targets on the failing seed vs 80 on the working one), the front arc (65 fully-valid acquisitions vs 4), nests missing when the tour is planned (4 exist at tick 1). The lesson: **reproduce the failure through the entry point the failing test uses, before forming any hypothesis.** `rollout_level` was visible in the source from the start.

- **⚠️ DECISION NEEDED: `broadcast.watch_threshold` (0.006) sits inside the ambient ATTENTION floor, so "look away to contain it" cannot hold.** Found 2026-08-05 while trying to promote the harness lane; `watching_the_feed_makes_it_generate_and_ignoring_it_stops` is red because the mechanic really is broken, not because the oracle is.

  **Measured.** Two screens, nobody deliberately looking at either, ambient ATTENTION sampled at each screen's own cell every 100 ticks:

  | tick | screen A | screen B | crabs |
  |---|---|---|---|
  | 100 | 0.00444 | 0.00130 | 40 |
  | 400 | 0.00741 ***** | 0.00611 ***** | 40 |
  | 600 | 0.00633 ***** | 0.00659 ***** | 41 |
  | 900 | 0.00618 ***** | 0.00666 ***** | 42 |

  (**\*** = at or above the threshold, i.e. counts as *watched*.) The field **rises to a resting plateau of ~0.0062–0.0067 and stays there**, so a threshold of `0.006` is under the noise floor: every screen counts as watched forever, and the feed generates regardless of where the player looks. Crab growth follows exactly.

  **Why the obvious history is misleading.** The *genome range* for this knob was already corrected once (2026-08-01, `(0.05, 0.80)` → `(0.0, 0.05)`) because the old band sat **above** anything the field reaches at a screen, making the anomaly permanently inert. That correction was right, but the shipped default landed at the opposite failure: `0.006` is at the bottom of the new band, below the ambient floor. The band `(0.0, 0.05)` therefore spans *both* pathologies, and only its upper part discriminates.

  **The watched side, now measured too.** Units pinned at each whole-metre stand-off from the screen, on a floor cell with real line of sight (`fog::update_los` reads unit `Transform`s, so pinning them drives the true mechanism), 240 ticks to settle:

  | stand-off | 0 m | 1 m | 2 m | 3 m | 4 m | 5 m | 6 m | 7 m | 8 m |
  |---|---|---|---|---|---|---|---|---|---|
  | screen's ATTENTION | 1.324 | 1.393 | 1.390 | 1.376 | 1.347 | 1.295 | 1.201 | 1.038 | 0.771 |

  8 m is the edge of `fog::VISION_RADIUS` (8 cells at `TILE_SIZE = 1.0`). The deposit is **binary per cell** over the line-of-sight set at `ATTENTION_RATE = 1.0/s`, settling at `RATE / evaporate`, so this is not a smooth distance falloff — it is a **step at the LOS boundary**, and the gentle decline across the row is the diffusion of a fixed deposit, not attenuation of the gaze.

  **So the discriminating band is `0.007 → 0.771`, a factor of ~110.** Any threshold in it separates "watched" from "ignored" cleanly. The shipped `0.006` is the one place it cannot: just below the diffusion floor. The evolvable band `(0.0, 0.05)` is therefore *mostly* correct — only its bottom sliver `(0, ~0.007)` is pathological, and the authored default landed in exactly that sliver.

  **`0.05` was tried, and it is not the fix — the threshold is not the root cause.** Applied together with its paired containment ceiling (`0.003` → `0.025`; the two are documented as moving together, and *both* shipped numbers sat inside the `0.0025–0.0067` diffusion floor, so the feed always generated **and** containment was a coin flip on where the squad stood). With that pair, `watching_the_feed_makes_it_generate_and_ignoring_it_stops` **passes** — and `the_watch_feed_fires_in_passive_play_on_the_held_in_seeds` starts failing, with the number that explains everything:

  | seed | nearest squad approach to a screen | peak ATTENTION at the screen |
  |---|---|---|
  | `0x5c09191` | 13.8 m | 0.009 |
  | `0x1ce5` | 15.1 m | 0.012 |
  | `0xfeed` | 13.1 m | 0.028 |

  **`broadcast.spawn_min_dist` is 16.0 tiles and `fog::VISION_RADIUS` is 8.** The anomaly is seeded at twice the distance its own mechanic can reach, and on every held-in seed the squad's closest pass is 13–15 m, so a screen is **never** in the line-of-sight set. The only thing that ever reaches it is diffusion. That makes the two oracles jointly unsatisfiable: a threshold above the noise floor makes the feed a prop (`emissions 0`), and a threshold inside the noise floor makes "look away to contain it" meaningless. `0.006` bought the second; `0.05` buys the first. Neither is the mechanic working.

  Reverted, so the shipped game is unchanged and no half-finished retune sits in the branch.

  **The decision is placement, not tuning.** Options, none costed: (a) bring `spawn_min_dist` under `VISION_RADIUS` — tried at `6.0` and screens then sit permanently in sight, so "ignoring" fails; the usable window is narrow and knife-edged, and `16.0`'s comment ("found, not handed over") is the intent it would give up; (b) make the squad's patrol actually visit the rooms screens are seeded in, which is where "the squad has to pull attention off the room" becomes real; (c) widen what the gate samples from the screen's own cell to a neighbourhood — but at 13 m the neighbours are equally out of sight, so this does nothing on its own; (d) raise `VISION_RADIUS`, which moves fog everywhere. **(b) is the only one that makes the mechanic mean what its doc says.**

  Note also that the genome band `(0.0, 0.05)` was corrected on 2026-08-01 on the strength of "~0.01 at 14 m" — which is the *out-of-sight diffusion* value, not a distance falloff. In line of sight the field reaches 0.77–1.39, so the pre-correction band `(0.05, 0.80)` was the one that spanned the real discriminating range. Whatever is decided, that band needs revisiting with these numbers.

  **This is a gameplay retune, not a bug fix:** it changes crab counts, therefore positions, therefore `snapshot_hash`, so it needs the replay gates and a golden re-pin on both architectures — and it shifts the meaning of the evolvable band, so baked elites measured under the old default were scored against an anomaly that always fired. Do not change it without picking a value deliberately.
- **Scope realism.** The XL items (H-3, I-1, K-4, C-6) are correctly sequenced last and behind prerequisites. Do not let the appeal of the "full vision" pull them earlier than M4, or the M0–M3 foundation slips.

---

*Housekeeping: `[SDT-00]`, `[PROB-ML]`, `[BAYESOPT]`, `[EPISTEMIC]` and (added 2026-07-27) `[PROG]` have empty catalog `title` fields in home-still — running `catalog_backfill_title` would make them discoverable by title. Not required to build; noted for corpus hygiene. `[PROG]`'s identity is confirmed from the PDF's own ACM reference block, not inferred.*