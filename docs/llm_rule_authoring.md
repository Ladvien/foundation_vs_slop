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

**The in-editor labeler (shipped 2026-08-06) is the harness for the asset half of this playbook.**
It lives in the mapper's Tiles tab — the mapper is dev-only and never ships, which is what
"stripped from release" means here — and it implements steps 1–3 plus the mount/surface half of
step 4 end to end:

- **Keys:** `L` photographs the focused piece (two 640 px booth renders, front and rear
  three-quarter) and asks the model; `Shift+L` walks everything missing judgement fields (press
  again to cancel); `U` applies the pending proposal through the ordinary edit path and commit
  door; `Y` discards it. Proposals render in the `SUGGEST` slate on the detail pane — chips
  ghost-lit, mount and note as `proposed:` lines — and the Tiles tab strip counts them.
- **Config:** the project root's **`.env` file** (gitignored — it carries the key), loaded on
  start, with the process environment overriding it for one-off runs. Variables:
  `EMERGE_VLM_KEY` (required), `EMERGE_VLM_URL`
  (default `http://127.0.0.1:9292/v1/chat/completions`), `EMERGE_VLM_MODEL`
  (default `qwen3-vl-30b`), `EMERGE_VLM_TIMEOUT_SECS` (120). For the local bmb model:
  `ssh -fN -L 9292:127.0.0.1:9292 bmb` then
  `echo EMERGE_VLM_KEY=$(ssh -n bmb 'cat ~/llm/.api-key') >> .env`. Ollama Cloud is a pure
  config flip: `EMERGE_VLM_URL=https://ollama.com/v1/chat/completions
  EMERGE_VLM_MODEL=qwen3-vl:235b EMERGE_VLM_KEY=$OLLAMA_API_KEY`. One endpoint per run; no
  fallback chain.
- **Script driving:** `echo <library_id> > labels.request` labels that entry through the exact
  production path; `echo clear > labels.request` empties the labeler (proposals, queues,
  in-flight, disk cache) — the devshot sentinel pattern, since captures cannot run headless.
- **Orientation is judged too:** the model proposes the item's `front` face (the camera geometry
  is stated in the prompt: image 1 shows the +X/+Z faces) — applied to `align.front` like any
  judgement — and flags a lying-down asset with the righting axis (`needs_turn`). A suggestion
  carrying `needs_turn` changes what `U` does: it performs the quarter turn through the same
  `tiles::rotate_mesh` the N/P keys run (its own undo entry, its own authored-cells guard),
  **discards the suggestion, and re-photographs** — the labels were judged from a sideways
  render, so applying them would bake the error in. The upright piece gets fresh labels; the
  human still presses every key.
- **The guardrails hold by construction:** the prompt is generated from the live `vocab.ron`
  (token names + notes), so the model is shown only the closed vocabulary; a suggestion carrying
  an unknown token is rejected WHOLE at arrival, naming the axis (with one automatic
  reprompt-on-rejection — see the research grounding below); out-of-vocab ideas exit only as
  flagged rows in `slop/llm/vocab_proposals.ron`, which nothing loads; applying goes through the
  same snapshot/record/persist idiom and `commit_measured` vocabulary gate as any hand edit; and
  suggestions persist in `target/vlm_suggestions.ron`, invalidated by re-export, manifest edit,
  or vocab retirement. Code: `crates/emerge-mapper/src/{vlm,label_booth,labels}.rs`.
- **Reference for LLM roles in games:** Gallotta et al. (2024), `10.48550/arXiv.2402.18659`.

## Research grounding for the labeler (measured choices — change them against the papers)

- **One automatic reprompt on rejection.** OVAL-Prompt (Tong et al. 2024) measured direct VLM
  affordance judgment as near-random (F 0.011), an LLM reasoning step at 0.392, and a
  reprompt-on-failure loop at 0.711 — competitive with supervised baselines. The labeler feeds the
  gate's rejection (axis + did-you-mean + the legal token list) back exactly once; the second
  verdict is final. Their second finding — VLMs stumble on uncommon class names — is why the token
  *strings* in `vocab.ron` should stay common words, with nuance in the notes.
- **Prompt-only JSON, no grammar constraint.** Format restriction helps closed-set classification
  (our axes) and hurts open reasoning (Tam et al., "Let Me Speak Freely?", EMNLP-Industry 2024);
  grammar-constrained decoding's advantage shrinks or inverts for ≥14B models given examples
  (Raspanti et al., ACL 2025, vs Geng et al., EMNLP 2023). **Revisit with llama-server GBNF only
  if the observed reject rate is high** — it would delete the malformed-JSON failure class.
- **`what` is the first schema key, deliberately.** Reasoning-first output orders avoid the
  accuracy drop answer-first ones suffer (Tam et al. 2024). Do not reorder the schema.

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
