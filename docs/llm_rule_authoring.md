# LLM rule-authoring playbook — `foundation_vs_slop`

How to use an LLM agent to **draft placement and behavior rules from the entities themselves**: point it at
one asset or one event, have it work out *what that thing is in reality*, and have it author *how it relates
to every other entity in the world* — as reviewable rules a human commits to `config.ron`.

This is the authoring side of the world-population grammar. For the architecture and its research grounding,
see `slop/research/2026-07-24-world-population-grammar.md`. For the rule types themselves, see
`src/placement/ir.rs` (constraints) and `src/placement/manifest.rs` (the manifest schema — today's primary
authoring surface). The Layer-A production grammar and the behavior/trigger vocabulary land with Stages B–C
of the design doc; until then, sections below that draft against them are explicitly marked **target-state**.

---

## The one idea: the LLM authors rules, it never runs them

The agent is a **dev-time, offline** tool. It produces RON text; a human reviews it; the deterministic solver
runs it. **The LLM is never in the simulation loop.** This is deliberate, and it is the whole reason the
approach is safe:

- The sim stays **one path, reproducible under a seed** — no runtime model call, no fallback, no nondeterminism
  smuggled into `snapshot_hash`.
- Following PCGRLLM (Baek et al. 2025, `10.48550/arXiv.2502.10906`): an LLM used to author **interpretable,
  weighted rule terms** — rather than to generate content directly — "preserves low-latency inference,
  mitigates direct LLM biases during sampling, and enhances stability and reproducibility." We author grammar
  productions, constraints, and triggers instead of reward terms; the argument is identical.
- The output is **readable** — the same elite-readability guard `world_genome`/`level_genome` rely on: a
  human can read the diff of rule dials and reject a bad one.

If you ever feel tempted to call the LLM at furnish time or sim time, stop — that violates the project's
one-path rule and breaks determinism. The agent's product is *source*, checked into `config.ron` like any
other authored config.

---

## Inputs and outputs (the contract)

**Input:** exactly **one** catalogue entry —
- an **asset**: its `ManifestItem` (GLB path, `tags`, `affordances`, `footprint`, `height`), optionally a
  rendered view of the GLB; **or**
- an **event/entity stub**: a name plus whatever gameplay component signature exists.
Plus **context**: the semantics already extracted for every *other* catalogue entry (so the agent can relate
the new one to them), and the list of predicates/roles the solver actually implements.

**Output:** typed RON the config loader already parses — `Candidate` / `Constraint` / `Production` / behavior
rules — **each with a one-line human-readable justification**. Never prose-only, never free-form.

---

## The pipeline (six steps per entity or event)

### 1. Perceive & identify — *what real-world thing is this?*
Read the entry. If it's an asset, optionally render the GLB and let the **`qwen3-vl-30b` VLM on `bmb`**
(already wired for CAD-render judging — see the global CLAUDE.md and `BEVY_GAME_INFO.md`) describe its shape,
colour, and parts. Produce a one-sentence real-world identification: *"a chest of drawers — a waist-high
storage cabinet with a flat top."*

### 2. Place it in the class hierarchy — *so rules inherit across kits*
Assign a semantic class and its ancestry (Tutenel's WordNet-style hierarchy, `10.1609/aiide.v6i1.12398`):
`chest_of_drawers ⊂ storage ⊂ furniture`; `tv ⊂ screen ⊂ appliance`; `nest ⊂ monster_home ⊂ structure`. A
rule authored on a **parent class** applies to every child in every asset kit — this is what keeps the
vocabulary extensible rather than merely long.

### 3. Extract the two axes — *keep "for" and "offers" separate*
This is the exact split that fixes the TV-on-bed bug. Emit two independent lists:
- **Services / affordances** — what the thing is *for*: `"sleep"`, `"store"`, `"emit"`, `"forage"`. (Fisher
  2012 / Qi 2018, already cited in `manifest.rs`.)
- **Features / surfaces** — what it *offers to others*: a surface class from the implemented vocabulary —
  today `"support"` (any prop-bearing top) and `"worktop"` (a desk/table top); the single source of truth is
  `furnish::SURFACE_CLASSES`. A bed affords `"sleep"` but offers **no** surface — so nothing rests on it.
  Never fold a surface token into the affordance list. **Proposing a new class** (`shelf`, `media`, …) is a
  two-part change: a row in `SURFACE_CLASSES` *plus* the manifest tokens — emitting the token alone fails
  loudly at load (`validate_manifest` rejects unknown surface classes), it does not silently no-op.

### 4. Relate it to the rest of the catalogue — *the core step*
Reading every other entry's semantics (from context), synthesize the relations that hold between this entity
and the others:
- **Support / stacking** — expressed **today** as the two manifest halves of one relation: the child gets
  `role: Scatter(surface: <class>)`, the parent lists that class in `surfaces` (Infinigen `StableAgainst`,
  `10.48550/arXiv.2406.11824`). A dedicated pair-predicate (`SupportedBy(child_tag, parent_tag)`) is
  **target-state Stage-C IR work** — until it exists, flag any need for it as a `Predicate::Custom` stub
  for a human, per the guardrails.
- **Spatial** — `Facing`, `Near`, `MinDistance`, `AgainstWall` (a seat faces a screen; a sink hugs a toilet).
- **Quantity / accessibility** — `Count` (one door per room); `Clearance` in front of appliances/food sources.
- **Behavior / triggers** (storylet precondition→effect, `10.1145/3337722.3337759`): "this trap fires when
  the squad enters"; "this food source respawns T seconds after depletion"; "this cache unlocks after the
  boss dies."
- **Set-piece production** — when the entity anchors a structure, propose a Layer-A grammar production:
  `nest → Lair(nest + 2..4 guards + hoard + choke)`.

### 5. Assign modality + weight — *hard vs soft, and a starting dial*
Each rule is `Hard` (must hold) or `Soft(weight)` (a cost term). Weights are **RL/QD-searchable** — the agent
proposes a *starting* value; the offline search (`level_genome`) then tunes it. Prefer `Soft` for aesthetic
relations (facing, grouping) and `Hard` only for correctness (reachability, no-overlap, one-door-per-room).

### 6. Emit typed RON + rationale
Write the rules in the exact schema the loader parses, each annotated. Today that schema is the
**manifest entry** (`ManifestItem` — what `config.ron`'s `placement.furniture.items` list holds):
```
// "A TV is a screen-class appliance; it rests on a support-class top and lights the room." (steps 1,2,4)
( key: "tv", glb: "kit/TV.glb", category: "appliance", tags: ["living"],
  role: Scatter(surface: "support"), footprint: (0.88, 0.30), affordances: ["emit", "screen"] ),
```
Region-level `Constraint`s in `ir.rs` scope over **candidate indices** (`Scope::Object(CandidateIx)` /
`Pair(ix, ix)`), which the furnish pass builds internally — tag-scoped constraint authoring
(`Pair("screen","seat")`-style) is **target-state Stage-C IR work**, not something to emit yet.

---

## Guardrails (each enforces an existing invariant)

- **Target tags / affordances / classes, never asset keys.** The portability invariant in `ir.rs` and
  `manifest.rs` ("matched, never interpreted"): a rule that names `"tv"` the asset instead of `"screen"` the
  class breaks the moment a kit is swapped. Reject such drafts.
- **Emit only predicates the solver implements.** Anything else must be a `Predicate::Custom` /
  `Role::Custom` token **explicitly flagged for a human to implement** — never a silently invented predicate
  the solver will ignore.
- **Human review is mandatory.** The agent *proposes*; a designer reads the readable diff, accepts or edits,
  and commits to `config.ron`. No auto-commit.
- **Fail loud at the door.** Drafted RON must pass `parse_manifest` / config validation and the full
  determinism suite (`cargo test`, and `cargo test --features test-harness` for anything touching sim state)
  **before** it ships. A bad draft errors at load, never at furnish/sim time. This includes the surface
  vocabulary: an unknown `surfaces`/`Scatter` token, or a scatter class no item in the kit offers, is a
  load-time reject naming the item (`validate_manifest`) — not a prop that quietly never spawns.
- **One path, no fallback.** If the agent can't confidently author a rule, it says so and leaves a flagged
  `Custom` stub for a human — it does not emit a degraded guess.

---

## Tooling

- **Models (local, on `bmb`, per the global CLAUDE.md service doc):** `gpt-oss-20b` or `qwen3-32b` for the
  text reasoning (steps 2–6); `qwen3-vl-30b` for the render look (step 1). Resolve `bmb`'s IP and API key as
  documented there; the service is LAN-only and API-key gated.
- **Harness:** a dev-only driver (`src/bin/` or a script) walks the catalogue, runs steps 1–6 per entry, and
  writes a candidate `config.ron` fragment for review. Dev-only — stripped from release, like `devshot`.
- **Reference for LLM roles in games:** Gallotta et al. (2024), `10.48550/arXiv.2402.18659`.

---

## Worked example

**Input:** asset `chest_of_drawers` (GLB, `footprint: (0.9, 0.5)`, `height: 0.8`), no rules yet.

**Agent output (for review) — the shippable part, in today's implemented schema:**
```
// 1–2: "A chest of drawers — waist-high storage cabinet; class storage ⊂ furniture."
// 3: for = store (+ its back belongs against a wall); offers = a flat prop-bearing top ("support").
// 4: its top hosts small scatter props (any prop with role: Scatter(surface: "support")).
( key: "chest_of_drawers", glb: "kit/Chest of Drawers.glb", category: "storage", tags: ["bedroom"],
  role: Freestanding, footprint: (0.9, 0.5), height: 0.8,
  affordances: ["store", "back_to_wall"], surfaces: ["support"] ),
```
(`back_to_wall` is how the implemented furnish pass expresses the wall relation today — it hardens the
`AgainstWall` predicate for that piece; see `furnish.rs`.)

**Target-state remainder (Stage C — flag for a human, do NOT emit as live config yet):** a distinct
`media`-class surface so designers can separate "TV-bearing" tops from generic shelves (one row in
`furnish::SURFACE_CLASSES` + the tokens), and a tag-scoped facing constraint
(`Pair("screen","seat") → Facing`) once the IR grows tag scoping. Until then a draft that wants them emits
a flagged `Custom` stub per the guardrails.

A designer reads this, maybe narrows the tags, and commits it. The deterministic furnish pass and the QD
search take it from there.
