# Foundation vs. Slop — Game State Review (2026-07-25)

Prepared for the game-loop design engineer. Synthesized from a full-codebase pass (5 parallel
subsystem investigations + direct reading of `main.rs`/`lib.rs`/`ui/`/`selection.rs`/`squad.rs`).
Every claim below was verified against current code, not assumed from memory of past sessions.

---

## 1. What this is, in one paragraph

A Bevy/Rust squad-tactics simulation set in an SCP-Foundation-flavored "Backrooms" dungeon. One
procedurally-generated level, a mouse-commanded MTF (Mobile Task Force) squad, and a small roster of
distinct anomalous "monsters" — all driven by **one shared emergent-AI substrate** (utility AI +
stigmergic pheromone fields + faction-partitioned fear) instead of bespoke per-enemy scripting. It is
architecturally closer to a systems-driven immersive sim / horde-survival sandbox than a scripted
horror game, and it is unusually mature for its size: a full offline reinforcement-learning /
quality-diversity (RL/QD) co-evolution pipeline exists to tune and curate its own content, and the
determinism-testing discipline is rigorous enough to catch cross-platform floating-point divergence.
**The biggest gap is that none of this currently resolves into a session with a beginning, middle, and
end** — there is no win/lose state at all.

---

## 2. Core game loop — what a player actually does today

**Boot flow** (`src/ui/state.rs`, `main.rs`): `AppState` is a strict linear machine —
`Boot → Title → Warmup → InGame`, **with no state after `InGame`.** Title (`ui/title.rs`) is a real
CRT-styled main menu ("New Run" / "Settings" / "Quit" — seed entry and "Continue"/save are explicitly
gated-future, per its own doc comment). `Warmup` exists purely so the player never watches the mycelia
mold finish colonizing the dungeon behind the loading screen. Then `InGame` — and it never leaves.

**Control scheme** (`src/selection.rs`, `src/squad.rs`): the whole squad is always-selected (no
click-drag box selection logic gating who's included); left-click ground-plane raycasts to a floor
cell, builds **one shared flow field**, and every unit gets a `MoveOrder` toward the same destination
— ORCA (reciprocal collision avoidance) packs them into a blob rather than each unit pathing
independently. This is a real-time, mouse-only, single-shared-destination RTS-lite scheme, not
per-unit micromanagement. Auto-aim/auto-fire appears to be the squad's only combat verb — there's no
manual attack-targeting system found.

**HUD** (`src/ui/hud.rs`): roster strip (per-unit color-coded health chips), a boss bar that appears
only once the Smiley is engaged (HP + calm/angry state text), and a time/speed readout tied to
`GameSpeed` (a time-control/pause system exists, `src/time_control.rs`). Player-adjustable HUD density
via a settings toggle. Entirely non-diegetic, no minimap yet (explicitly flagged "later phases" in the
module doc).

**Death handling exists, but termination doesn't.** `squad.rs` (~line 778) is explicit and deliberate:
"every unit can die, including the last: a total wipe is a real outcome... the zero-unit world is
well-defined" — `pick_leader` no-ops, `cohesion::update_anchor` clears the squad anchor cleanly. But
**nothing consumes that state to end the session.** A wiped squad just sits there as zero entities;
there is no `GameOver`/`Victory` `AppState`, no restart prompt, no return-to-title trigger. The RL/QD
search's own fitness function (`squad_ai::qd`) explicitly scores "the squad was not wiped" as a gate —
so the concept of a loss condition is already load-bearing offline, it's just not wired to end anything
live. This is the single highest-leverage open design decision: **what happens when the squad wipes,
and is there ever a way to "win" a level and progress?**

**No mission/level structure.** One dungeon is generated per boot; there's no level-to-level
progression, no save/load, no unlocks, no objectives beyond implicit survival. This is consistent with
how the RL/QD side treats each seed as an independent scored episode — the game currently *is* that
episode, exposed directly to a human player with no wrapper.

---

## 3. Creature ecosystem & emergent AI (the systemic core)

**Roster and each one's behavioral thesis:**

| Creature | Core mechanic | Files |
|---|---|---|
| Crabs | ~40→5000 wall-climbing swarm; individually trivial, lethal only in mass; forage meat gibs → haul to nest → breed on delivery | `src/crab/`, `src/nest.rs` |
| Smiley | Single slow bullet-sponge; conceals overwhelming power unless "observed"; camera-gaze-gated flee/zap reflex | `src/enemy.rs` |
| SCP-999 | The one *benign* creature — seeks the most-anxious squad member, drains FEAR / raises MORALE on contact | `src/scp999/` |
| SCP-1048 (Builder Bear family) | Unshootable benign original **builds 3 hostile copies while unobserved** — "watch the harmless one" is the actual play loop | `src/scp1048/` |
| SCP-150 mancae | Body-horror parasite: stalk → burrow → gestate → burst (not instakill); hosts both squad *and* crabs | `src/parasite.rs` |
| Almond Water | Not a creature — a self-regenerating healing resource using the same field-diffusion kernel as pheromones | `src/almond_water/` |
| Mold/Mycelia | **Two separate implementations, easy to conflate**: `mold.rs` is the deterministic CPU gameplay field (photophobic, QD-tunable, affects balance); `mycelia/` (8,251 lines, the single biggest subsystem) is a GPU cosmetic Physarum/Gray-Scott mirror that **explicitly never touches gameplay**. Anyone tuning balance must edit `mold.rs`. | `src/mold.rs`, `src/mycelia/` |
| SCP-610 "Infected" | **Fully asset-built and verified (rig, clips, tri budget) with zero game-side code** — no faction entry, no brain, not spawned anywhere. The most "shovel-ready" unbuilt content in the repo. | `assets/scp610` and adjacent lore docs only |

**Stigmergy substrate** (`src/ai/field.rs`) — the game's most novel systemic idea: 10 scalar channels
over the dungeon grid (`SCENT`, `THREAT_GUN`, `CRAB_DENSITY`, `MEAT`, `ALARM`, `THREAT_CRAB`,
`THREAT_ANOMALY`, `NOISE_SQUAD`, `NOISE_SWARM`, `ATTENTION`) that every faction deposits into,
diffuses, and reads from — a shared substrate rather than per-agent perception. A structural invariant
(enforced via `Faction`, tested) is that **nothing may fear a channel it emits** — squad and crab
threat/noise channels are kept as separate faction-partitioned pairs specifically because an earlier
undifferentiated channel made the squad flee its own gunfire. `ATTENTION` (gaze-as-pheromone) is read
with **opposite sign by different readers** — mold recoils from being watched (SCP-173-style), while a
marked predator is drawn toward it (SCP-096-style) — that per-reader sign-flip is the extensibility
pattern for adding new gaze-reactive content. `RallyField` is the one vectorial (not scalar) channel —
direction toward live prey, laid by crab scouts.

**Utility-AI brain** (`src/ai/`): one shared decision engine — `Drives` (FEAR/HUNGER/MORALE, etc.) feed
`utility.rs`'s dual-utility scoring (rank bucket, then weighted-random within rank) to produce a `Mode`.
Per-creature "brains" are data literals in this shared framework, not bespoke engines, except squad
roles which additionally get RON-authored repertoires so the offline QD search can evolve them.

**Unexploited opportunities already paved by the architecture:**
- `NOISE_SQUAD`/`NOISE_SWARM` (an audible-din perception channel) exists but has no clear payoff
  mechanic yet — a stealth/noise-discipline layer is half-built and unused.
- Almond Water's resource-contest kernel is identical to the pheromone kernel — any future contested
  resource (territory, corpses, light) is a new `FieldId` away, zero new engine work.
- The SCP-150 three-body web (parasite infects both squad *and* crabs) is architecturally rare — most
  games don't let one hostile faction parasitize another — and is a strong lever for emergent
  inter-faction conflict content.
- `Faction` currently caps at 4 (Foundation/Crab/Anomaly/Bear) with a tested dense-index invariant —
  adding a 5th (e.g. a rival human faction) is a well-paved path.

---

## 4. Procedural generation stack

**Pipeline, in order:**
1. **Coarse WFC** (`src/wfc.rs`) — Wave Function Collapse over a room-connectivity graph
   (min-entropy observe + arc-consistency propagation, citing Karth & Smith 2017 and Kim et al. 2020);
   fails loudly on an unconvergeable contradiction rather than degrading.
2. **Fine-grid expansion** (`src/dungeon.rs`, ~3,000 lines) — turns collapsed room slots into an
   actual 1m-tile grid: walls, doorway clearance, and a view-relative "knee-wall cutaway" so the fixed
   isometric camera can always see into rooms. Single source of truth for walkability/collision/fog.
3. **Furniture placement grammar** (`src/placement/`) — a genuinely research-grade, engine-free
   constraint system: an IR (`Role`/`Predicate`/`Modality`/`Outcome::Partial`) with zero `bevy::`
   imports, routed to three solver backends by declared capability — a WFC solver for tiled scatter, a
   **Metropolis-Hastings simulated-annealing solver** (adapting Merrell, Schkufza, Li, Agrawala & Koltun
   SIGGRAPH 2011) for freestanding furniture layout, and a hard-constraint solver for global counts (one
   door per room, etc). Affordances ("what a piece is for") are separated from surfaces ("what a piece
   offers"), citing Tutenel et al. 2010. Determinism: one seeded ChaCha8 stream split per-region via
   SplitMix64, so regions solve independently and reproducibly regardless of ECS iteration order.

**QD-evolved level generation** (`squad_ai::level_genome/level_search/level_eval/level_quality`): a
4th MAP-Elites population (~30 typed genes — architecture, furniture density, mushroom-habitat params)
scored by expressive-range analysis (Smith & Whitehead 2010): a hard minimal-criterion gate
(connectivity ≥99.9%, ≥2 rooms, floor-fraction band) then a weighted fitness blending connectivity
margin, room-count/size-variance bands, furniture density, and mushroom coverage. Descriptor axes are
clutter × infestation (an openness axis was tried and dropped — too little variance in this liminal
style to be a useful archive dimension).

**Unfinished:** `dungeon.rs`'s split into a submodule is deliberately deferred (its collision/generation
code is interleaved inside one `impl` block, risky to split mid-block); one `assert!` in `wfc.rs`
should be a loud `Result` instead; unclear whether the graph-topology WFC front-end
(`collapse_graph`) is actually wired into live generation or exists only as a tested library capability.

**Opportunity:** the `Region`/`Candidate`/`Constraint`/`Predicate` grammar never assumes "room" — the
same IR + solver trio could place combat encounters (cover/spawn/turret layout) or anomaly siting with
zero new solver code. And since a level is already a fitness-scored, descriptor-indexed genome, a live
director could pick an archive cell matched to the current player's skill/session — QD-driven dynamic
pacing instead of a fixed seed.

---

## 5. RL/QD evolutionary infrastructure & testing

**Six MAP-Elites populations**, all in `src/squad_ai/` (37 files, 13,244 lines — the single biggest
module in the codebase), all gated behind `test-harness` and driven by a separate `train` binary
(`cargo train {bench,evolve3,levels,audio,behavior,rl,poet}`) — **none of this compiles into the
shipped game binary**:

1. `world_genome` — field-propagation/sim-dynamics tuning
2. `behavior_genome` — 89-knob locomotion/steering/combat-cadence vector
3. `audio_genome` — acoustic propagation + per-faction perception gains
4. `level_genome` — dungeon/furniture/mushroom density (see §4)
5. `policy_genome` — the actual RL slot: an MLP (`NeuralPolicy`) evolved via **CMA-ES**
   (neuroevolution, not gradient RL)
6. `poet` — Wang/Lehman/Clune/Stanley 2019 POET: co-evolves world *and* squad genome together with a
   learning-progress curriculum

Most share one fitness core: `surprise::fitness = W·S·L` (witnessed × surprising × learnable — an
anti-noisy-TV, anti-degenerate-difficulty objective), gated by a POET-style minimal-criterion admission
test.

**Does evolved content reach the shipped game?** Yes, but **opt-in only** — `elite_overlay.rs` reads
env vars (`FVS_BEHAVIOR_ELITE`, `FVS_WORLD_ELITE`, `FVS_AUDIO_ELITE`, `FVS_LEVELS_ELITE`,
`FVS_POLICY_ELITE`) and overlays one archive cell onto the loaded config at startup, failing loudly on
a bad archive. `config.ron` — what actually ships by default — is hand-authored, not sourced from any
evolved archive. Compounding this right now: `MODE_COUNT` recently grew (25→29, SCP-1048 landing), so
every checked-in `elites_*.ron` is stale and rejected until a multi-hour `cargo train rl`/`prior`
rerun happens (tracked in BACKLOG, deliberately deferred).

**Testing philosophy** (`TESTING.md`) — a two-altitude model: gameplay logic (AI/movement/combat/WFC)
is bit-reproducible → exact-hash golden tests on `SimConfig::deterministic_core()` (physics off);
Avian physics (gib chunks only) is not bit-stable → liveness/tolerance oracles only; render/FX → SSIM
perceptual comparison. The headline documented trap: **ECS query order is not a stable total order**
— any sort whose key can tie (especially a key that's a prefix of the sorted value) silently falls
through to arbitrary per-process order. This caused three real historical bugs and is now mechanically
enforced (`sort_total!` / `sort_value_canonical` / `// SORT-OK:`, checked by
`tests/determinism_lint.rs`). Companion rule: a determinism probe on an idle box proves nothing — races
only surface under CPU load over long episodes.

**Known fragile/unfinished pieces:** CMA-MAE emitter (the SOTA MAP-Elites upgrade) is implemented and
unit-tested but unreachable from any `train` subcommand — dead code under the project's own "one path"
rule; `RemotePolicy` (live external trainer hook) exists but nothing drives it; harness CI lane is
`continue-on-error`, not a hard gate, despite being GPU-free now; policy archives are presently stale
(see above).

**Biggest opportunity:** the infrastructure already computes "would a player find this genuinely
surprising, learnable, and witnessed" per config — a content-quality oracle, not just a balance tool.
Nobody has wired POET's world×agent co-evolution as a **live per-playthrough curriculum generator**
yet — it runs offline today. That would produce emergent, non-scripted enemy/squad personalities that
vary by save rather than one fixed brain, a genuinely uncommon feature at this project's scale.

---

## 6. Narrative, content, and the full BACKLOG punch-list

**Lore is aspirational, not implemented.** `docs/lore/` contains four research-reference docs (universe
bible with a multiversal Hub/Branch/Floater structure and Hume/EVE/Akiva power meters; a 5-axis
personnel/role taxonomy for a 4-role party; an equipment taxonomy; an Almond Water "belief is the
mechanic" design vet). **None of this touches the shipped code.** The actual game is a single-site
squad-vs-swarm horror-sim with one undifferentiated 5-member squad — it has fully diverged from the
Foundation-bureaucracy/multiverse/party-role framing the lore docs describe. Worth a deliberate decision:
is the lore doc still the long-term thesis, or has the shipped game already become its own thing?

**Dialogue** (`src/dialogue/`, 1,448 lines): a real but thin system — world-space billboarded
speech/thought bubbles, an authored RON conversation-graph model, a runtime state machine that freezes
input during a chat. Two parallel feed systems exist, but the squad's observation-driven
persona/bark generator (`squad_ai::dialogue`) ran with **zero readers for its whole life** until
recently wired up; the authored script corpus is **exactly one conversation**, triggered only by a dev
hotkey. Scaffolding, not content.

**BACKLOG.md (838 lines) — categorized:**

- **Open known bugs:** Smiley shader/mesh clips through walls; squad gets stuck in doorways with units
  already in the doorway (flowfield lookahead limitation).
- **Fixed-but-undocumented (README stale):** wall corner post-gap, lamps floating, trashcans-in-couches
  (needs visual re-confirmation).
- **Design decisions explicitly blocked on the user** (README vs. code disagree — do not resolve
  without a decision): (1) Smiley's "observed" definition — README says squad-member line-of-sight,
  code uses player-camera-gaze; (2) crab "numbers-kill >5" gate — README claims a hard threshold, code
  has none (a lone crab already deals real DPS); (3) the same gate for pounce — paired with #2, "one
  design choice, not two."
- **Deferred refactors:** split `dungeon.rs` (~3,040 lines) and finish half-done splits of
  `mycelia/mod.rs`, `almond_water/mod.rs`; split `coevolve.rs` (1,353 lines); extract RON-splice/golden
  machinery out of `bin/train.rs` (2,563 lines); remove orphaned assets (`hazmat/`,
  `hazmat_locomotion_pack/`, `kenney_blaster-kit_2.1/`); dedupe shader noise functions.
- **Process/testing hardening gaps:** harness CI lane is advisory, not a hard gate; no clippy denylist
  against `unwrap`/`expect`/`panic`/`unsafe`; no macOS/ARM CI lane despite an already-hit ARM↔x86 f32
  divergence bug; CMA-MAE unreachable dead code (see §5).
- **The structural gap worth flagging loudest:** CLAUDE.md's own rule — "every feature added is
  correctly included in the RL/QD systems for evolving" — is convention, not a lint. Concretely:
  `GoreSettings.autogib_*` (6 knobs) already tipped a 5/5 win into a wipe from a mesh swap alone (a
  seed had to be retired over it), yet **no genome evolves gore.** Also un-evolved: `MetropolisWeights`
  (10+ furniture-placement knobs), most of `PerceptionTuning`'s sight thresholds, crab/parasite swarm
  cadence.
- **Content asset packs, status:** SCP-1048 A/B/C family fully wired and live (A carries 2.6× the
  triangles of its siblings from 3 dead UV sets — the cheapest asset win available and a documented
  test-suite slowdown cause); SCP-150, SCP-999, dimensional_crab (the swarm asset), 16-species
  death_cap/mushroom system — all wired. **SCP-610 "Infected" is fully asset-built and verified with
  zero game-side wiring** — the single most shovel-ready content gap in the repo. `hazmat`/
  `hazmat_locomotion_pack`/`kenney_blaster-kit_2.1` are orphaned dead weight.

**Most conspicuously missing, narrative/content-completeness:**
- No title-to-ending arc: no win condition, no game-over screen, no mission/level-select structure
  (confirmed directly in `ui/state.rs` — see §2).
- No player-character identity or role differentiation — the squad is one undifferentiated unit type;
  the lore docs' 4-role party concept has no code counterpart.
- No campaign/progression — no save/load, no unlocks; every run is one procedurally-generated episode,
  consistent with how the RL/QD side treats a seed, but currently exposed to the player with no wrapper.
- Dialogue has essentially no authored content and no gameplay trigger.

---

## 7. Ranked opportunities for a game-loop design engineer

1. **Decide and implement session termination.** This is the one gap blocking "is this a game yet."
   The zero-unit wipe state is already well-defined in the sim (`squad.rs`); it just needs an
   `AppState` transition (`GameOver`/`Victory`) and a restart/return-to-title path. Doing this also
   forces the win-condition question (survive N minutes? clear the level? extract?), which in turn
   gives the procgen and RL/QD systems something concrete to target.
2. **Resolve the three README-vs-code design forks** (Smiley gaze source, crab/pounce numbers-kill
   gate) — these are blocking both a correctness fix and the accuracy of the game's own pitch document.
3. **Wire SCP-610** — a complete, verified asset sitting unused is the cheapest large content addition
   available.
4. **Turn the offline POET/QD pipeline into a live curriculum director** — the biggest structural
   differentiator this codebase has over a typical horde-survival game, currently inert at runtime.
5. **Give the stigmergy substrate's unused channels (`NOISE_SQUAD/SWARM`, `ATTENTION` sign-flip
   pattern) a payoff mechanic** — a stealth/noise-discipline layer and a second gaze-reactive creature
   are both architecturally free.
6. **Close the "every feature must evolve" gap for gore/placement/perception tuning** before it causes
   another retired seed.
