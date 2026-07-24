# A world-population grammar — one rule system for furniture, monster homes, triggers, food, and loot

**Trigger:** the furniture placement work (`slop/research/2026-07-24-placement-rule-unification.md`) fixed a
real bug (TVs on beds) and unified *furniture* placement onto the constraint IR. The broader question it
raised, and the one this doc answers: **what is the best-practice way to author rules that combine to
produce emergent, interesting-for-the-player behavior across every kind of world entity** — furniture,
monster homes, event triggers, food sources, acquireable items — **and can an LLM draft those rules from the
entities themselves?**

This is a design/research doc, not an implementation. It is the reference behind the roadmap; the staged code
lives in the plan and in `src/placement/`. Everything cited here was read from the local home-still corpus.

---

## 1. The question, restated: how do you get "lots of interesting things happening"?

The literature has a precise answer, and it is not "write more content."

Juul (*The Open and the Closed: Games of Emergence and Games of Progression*, `10.26503/dl.v2002i1.9`) splits
all games into two structures:

- **Progression** — challenges presented serially; the designer scripts the sequence. High authorial
  control, low replayability, "on rails," needs a *walkthrough*.
- **Emergence** — "a small number of rules that combine and yield large numbers of game variations, which the
  players then design strategies for." High replayability, fosters *strategy guides*, and produces variation
  "neither anticipated by the game designer, nor easily derivable from the rules."

Emergence is the primordial structure and the one that produces the "I've never seen that" moment. The design
lever is therefore **composable rules**, not hand-placed encounters. Harvey Smith's distinction (quoted in
Juul) matters too: *desirable* emergence (rules interact into interesting play) vs *undesirable* emergence
(exploits like proximity-mine climbing). You cannot get the first without risking the second, so emergence
must be **evaluated**, not merely generated — which is where quality-diversity (§5) comes in.

Chauvin et al. (*An Out-of-Character Approach to Emergent Game Narratives*, `fdg2014_fdg2014_wip_05`) name the
five properties a rule-based world must protect to stay interesting rather than merely random: **coherence,
agency, possibility space, uncertainty, co-authoring**. The whole architecture below exists to *widen the
possibility space while keeping coherence* — the two pull against each other, and the layered split is the
standard way to hold both.

**So the deliverable is a grammar of composable rules + a way to measure whether their emergent product is
interesting.** The rest of this doc is how the corpus says to build exactly that.

---

## 2. The convergent architecture: generate-then-solve, evaluated by quality-diversity

Three independent literatures — grammar-based PCG, constraint-based PCG, and quality-diversity — converge on
one pipeline. It is **layered**, and this game already has three of the five layers built.

| Layer | Decides | Canonical sources |
|---|---|---|
| **A — Generative grammar** | *what* set-pieces exist and their rough structure | mission/space graph grammars (PCG Book ch. 3 & 5, `pcgbook_chapter03/05`); weighted rewrite formalism `10.1145/3102071.3102079`; two-step structure-then-populate (Font et al., in the dungeon survey `10.5753/jis.2021.999`) |
| **B — Constraint IR + solvers** | *where* exactly (geometry) | Sturgeon `10.1609/aiide.v18i1.21944`; Infinigen Indoors `10.48550/arXiv.2406.11824`; ASP for PCG (PCG Book ch. 8, `pcgbook_chapter08`); Tutenel `10.1609/aiide.v6i1.12398` |
| **C — Behavior / triggers** | *when / if* (precondition → effect) | storylets / Lume `10.1145/3337722.3337759`; emergent-narrative vs drama-manager `10.1145/1822309.1822325` |
| **D — QD evaluation / search** | *is it interesting?* | PCG-QD `10.1109/CIG.2019.8848053`; MAP-Elites `10.48550/arXiv.1504.04909`; surprise `10.1145/3102071.3110577`; expressive range `10.1145/1814256.1814260` |
| **E — LLM authoring aid** | drafts A/B/C rules offline | LLM-authors-weighted-terms (PCGRLLM `10.48550/arXiv.2502.10906`); LLM-in-games survey `10.48550/arXiv.2402.18659` |

### 2.1 Layer A — the generative grammar (what + rough structure)

Grammars generalize past strings to **graphs, tile maps, and shapes** (PCG Book ch. 5): "graphs are more
useful than strings to represent missions and spaces." The mission/space split is the key idea — a **mission
grammar** governs flow, pacing, and causality; a **space grammar** governs connectedness and layout; the two
are generated so they reinforce each other. A production is a rewrite rule: `Start → task … lock–key … boss →
goal`, with wildcards and containment edges.

A rigorous, implementable formalism comes from the parallel-programming-puzzle generator
(`10.1145/3102071.3102079`): a rule is a tuple `⟨P, P⁻, R, l, t, w, d⟩` — a **pattern** `P` that must match,
a **negated pattern** `P⁻` that must *not* match, a **replacement** `R`, an **application limit** `l`, a
**tag** `t` to scope which rules fire, a **weight** `w` for stochastic selection, and a distance `d`. This is
enough to author "a lair spawns 2–4 guards but never two treasure hoards," and its weights are exactly the
RL/QD dials of Layer D.

The controllability lesson (PCG Book ch. 5): don't apply rules indiscriminately (a naive "add task" rule has
no upper bound). Break generation into steps — "one step to create a highly randomised graph, a second to
restructure it into something that makes sense." Font et al. (dungeon survey `10.5753/jis.2021.999`) apply
exactly this for the entities we care about: **generate the dungeon structure first, then populate the
detailed, expensive elements — monsters and chests — afterward.** That two-step is our grammar → IR seam.

### 2.2 Layer B — one weighted constraint problem, many backends (where, exactly)

This layer already exists in `src/placement/ir.rs` + `solver.rs`; the corpus validates its shape and tells us
what to add.

- **Sturgeon** (`10.1609/aiide.v18i1.21944`) is the closest architectural match: a tiny mid-level API
  (`MAKEVAR`, `MAKECONJ`, `CNSTRCOUNT(vars, lo, hi, weight)`, `CNSTRIMPLIESDISJ(lit, disj, weight)`, `SOLVE`)
  that compiles to several low-level solvers (SAT / SMT / Answer Set / portfolio). **Every constraint carries
  a weight, so hard and soft are one mechanism** — our `Modality::Soft(f64)` *is* Sturgeon's weight. Its four
  design-rule families — **tile, pattern, distribution, reachability** — are worth stealing as a checklist,
  and **reachability is formulated as a graph problem translated into constraints** (a start→goal path must
  exist), which is how "the loot room must be reachable" or "don't wall off the boss" become rules rather
  than post-hoc fixes.
- **Infinigen Indoors** (`10.48550/arXiv.2406.11824`) gives the constraint *families*: **symmetry, spatial
  relation, quantity, physics (no overhang), accessibility (free space in front of appliances)**. Its
  relation primitive `StableAgainst(child_tag, parent_tag)` is the correct generalization of "TV on a media
  surface" — a relation over two tags, not a new bit — and is Stage A's `Predicate::SupportedBy`. Its solver
  is greedy simulated annealing, hierarchical (floor plans → large furniture → small objects); our
  `MetropolisSolver` is the same. Its stated goal — "separate constraint specification from constraint
  solving" — is the seam `ir.rs` was built around.
- **ASP for PCG** (PCG Book ch. 8, `pcgbook_chapter08`) states the composition principle directly: "by
  building on combinations of simpler constraints and rules, complex constraints can be formulated that lead
  to the **emergence of interesting level-design properties**." Reachability is expressed recursively.
  Horswill & Foged's *Fast procedural level population with playability constraints* (cited there) is the
  direct precedent for "populate the world under playability constraints."
- **Tutenel** (`10.1609/aiide.v6i1.12398`) supplies the semantic discipline the furniture doc adopts:
  objects carry typed **features** (`top`, `front`, `storage`) distinct from **services** (what the object is
  *for*), plus a **class hierarchy** so a rule declared on `Screen` applies to every TV/monitor in every kit.

### 2.3 Layer C — behavior as precondition → effect (when / if)

"Placement + behavior" means event triggers, food respawn, and loot gating are *rules*, not bespoke systems.
The storylet model is the fit. Lume (`10.1145/3337722.3337759`) builds interactive narrative from
**storylets**: short units, each **selected by constraint satisfaction over world state**, with
author-specified **preconditions** and **effects** (it uses Prolog as the built-in constraint solver). This
is the same declarative shape as Layer B — a guard over state plus an effect — so triggers live in the same
IR: our currently-unused `ir::Guard(String)` becomes the precondition slot.

The design tension to respect (Riedl, `10.1145/1822309.1822325`): pure **emergent narrative** maximizes
believability/possibility-space but "may or may not have the desired features"; a **drama manager** guarantees
structure but can feel forced. The resolution both Riedl and Chauvin reach is a *hybrid* — a light,
optional structuring layer over an emergent base. In our terms: the **grammar (Layer A) is the drama manager**
(it guarantees a coherent set-piece skeleton), while the **triggers + simulation (Layer C + the ECS) are the
emergent base**. That is precisely the layered split, and it is why we don't script encounters directly.

### 2.4 Layer D — quality-diversity: measure the emergence (is it interesting?)

Emergence is only *desirable* emergence if evaluated. The game already has an unusually complete QD stack;
the corpus is the theory under it.

- **PCG-QD** (`10.1109/CIG.2019.8848053`) surveys quality-diversity for content: don't optimize a single
  point, **illuminate a whole archive** of diverse-but-good artifacts. Its techniques — **constrained
  MAP-Elites** (feasible/infeasible two-population), **novelty search with local competition**, and
  **surprise search** — are exactly what `squad_ai` already runs. Descriptors used in the survey (playthrough
  properties, triggered mechanics, design patterns) are the same *kind* of descriptor as our run signatures.
- **MAP-Elites** (`10.48550/arXiv.1504.04909`) is the archive itself (`squad_ai::qd`).
- **Surprise, cognitively** (`10.1145/3102071.3110577`): the VCL model — Violation of expectations, Caught
  off guard, Learning — mirrors `surprise.rs`'s Bayesian-surprise + learnability construction.
- **Expressive range** (Smith & Whitehead, `10.1145/1814256.1814260`) is the replayability measure already in
  `replayability.rs`: a generator is good if its *runs* spread across the descriptor space, not if one tuned
  point plays the same every time.

The consequence for this design: **grammar-production weights and constraint weights become genes** in
`squad_ai::level_genome`, and the emergent world they produce is scored by the existing
surprise/interest/replayability proxies. This is Sturgeon's thesis (learned + designed constraints in one
weighted problem) and the CLAUDE.md rule ("wire every feature into RL/QD") satisfied by construction.

### 2.5 Layer E — the LLM as an offline author of weighted rules

The safest, best-supported use of an LLM here is **not** as a runtime generator. PCGRLLM
(`10.48550/arXiv.2502.10906`) makes the case precisely: use the LLM "not as a generator of content but as a
**designer of reward functions** … mapping high-level preferences (validity, diversity, novelty, safety,
style) into interpretable reward terms with tunable weights," which "preserves low-latency inference,
mitigates direct LLM biases during sampling, and enhances stability and reproducibility." Transpose "reward
terms" to "grammar productions + constraints + trigger rules with weights" and that is our Layer E exactly:
the LLM **authors the rule text; the deterministic solver runs it.** The reproducible, one-path sim is
untouched. Roles-of-LLMs-in-games are surveyed in `10.48550/arXiv.2402.18659`. The full agentic pipeline is
its own document (`docs/llm_rule_authoring.md`).

---

## 3. What the game already has (so we build the gap, not the whole thing)

- **Layer B — built.** `ir::PlacementProblem` (candidates + constraints), `solver::Orchestrator` routing by
  `Role` to `WfcSolver` / `MetropolisSolver` / `ConstraintSolver`. `ir::Region` is *already* domain-agnostic
  ("interiors map it to rooms, urban to parcels, dungeon to cells"), so it generalizes past furniture with no
  type change. The `Predicate` set (`Clearance`, `AgainstWall`, `Facing`, `MinDistance`, `Near`, `Count`,
  `Aligned`, `Custom`) already covers most Infinigen families.
- **Layer D — built, and strong.** `level_genome.rs` evolves level-generation config as typed genes over a
  shipped base (readable elites); `world_genome.rs` evolves world dynamics (POET-style, `arXiv:1901.01753`);
  `surprise.rs` = witnessed learnable-surprise `W·S·L`; `replayability.rs` = expressive-range spread;
  `qd.rs` = the MAP-Elites archive.
- **The gap is Layers A, C, and E**, plus routing the existing hand-rolled placements through B. The four
  greedy spawn scans (`enemy::spawn_enemies`, `crab::setup`, `scp999`, `nest`) are one algorithm —
  far-from-spawn + spread — i.e. `MinDistance` + `Count` over `Scope::Region`, which `ConstraintSolver`
  already solves.

---

## 4. The rule vocabulary (sketch)

One vocabulary spans all five entity classes. Concretely (RON-ish, authored in `config.ron`):

```
// Layer A — a production: a non-terminal rewrites into a subgraph of tagged nodes + relations.
Production(
  head: "Lair", tag: "monster_home", weight: 1.4, apply_limit: 3,
  guard: Some("region.area > 40"),                 // where this set-piece may appear
  body: [
    Node("nest",  count: One),                     // the monster home
    Node("guard", count: Range(2, 4)),             // its defenders
    Node("hoard", count: One, tag: "loot"),        // acquireable items
    Relation(SupportedBy(child: "hoard", parent: "nest_shelf")),
    Relation(MinDistance(a: "guard", b: "nest", m: 1.5)),
    Trigger("alarm", when: "squad_enters(region)", effect: "wake(guard)"),
  ],
)

// Layer B — a constraint (already an ir::Constraint), targeting tags/affordances, never asset keys.
Constraint(scope: Pair("seat","screen"), predicate: Facing, modality: Soft(1.5))

// Layer C — a trigger/behavior rule: precondition (guard over world state) → effect.
Behavior(on: "food_source", available_when: "consumed && elapsed > respawn_s", effect: "respawn")
```

Every rule targets **tags / affordances / classes, never asset keys** — the portability invariant already in
`ir.rs` and `manifest.rs` ("matched, never interpreted"). The grammar lowers a derived content graph into
per-region `PlacementProblem`s (Layer B) plus behavior components (Layer C); the orchestrator and QD stack
are untouched.

---

## 5. Worked example: a "Larder" makes several systems one rule

Today, almond-water springs, mold seed sites, nest seating, and enemy spawns are four hand-written placement
routines. As grammar + constraints:

- **Production** `Larder → food_source×2 + forager_nest + cover` with `MinDistance(food_source, spawn) > D`
  and `Near(forager_nest, food_source)`.
- **Constraint** (accessibility, Infinigen family): `Clearance` in front of each `food_source` so the swarm
  can actually reach it.
- **Trigger**: `food_source.available_when(!depleted); on_depleted → respawn after T`.

Now a designer (or the LLM aid) tunes one weighted rule set instead of editing four `.rs` files, the QD search
evolves the weights, and `surprise`/`replayability` report whether the resulting encounters are actually
varied and interesting — the honest argument for the unified vocabulary. This is the same argument the
furniture doc made ("we are writing the fifth hand-rolled placement rule while a solver-routed IR sits unused
next to it"), extended from props to gameplay.

---

## 6. Roadmap

Staged so each step is independently shippable and each retires a hand-rolled mechanism (full detail in the
implementation plan):

- **Stage A** — split `surfaces` from `affordances`; add `Predicate::SupportedBy`; route furnish Pass 4
  through the orchestrator. *Fixes the TV-on-bed bug; closes the IR bypass.*
- **Stage B** — compile the four greedy spawn scans into `MinDistance`+`Count` constraints (behind the
  determinism/liveness gates).
- **Stage C** — `src/placement/grammar.rs`: the weighted graph-rewrite grammar (Layer A).
- **Stage D** — promote `ir::Guard` into the storylet precondition→effect vocabulary (Layer C).
- **Stage E** — `GrammarGenes` in `level_genome.rs`; encounter-graph descriptor in `qd.rs` (Layer D hookup).
- **Stage F** — the offline LLM authoring agent + `docs/llm_rule_authoring.md` (Layer E).

**Determinism is non-negotiable** for Stages B–E: anything touching pinned sim state (`snapshot_hash`) needs a
stable total-sort key (`sort_total!` / `util::sort_value_canonical` / `// SORT-OK`) and lives on
`FixedUpdate`; verify a seeded replay reproduces before shipping (`TESTING.md`).

---

## References (all read from the local home-still corpus)

- Juul (2002). *The Open and the Closed: Games of Emergence and Games of Progression.* `10.26503/dl.v2002i1.9`
- Chauvin, Levieux, Natkin, Donnart (2014). *An Out-of-Character Approach to Emergent Game Narratives.*
  `fdg2014_fdg2014_wip_05`
- Togelius, Shaker, Dormans. *Procedural Content Generation in Games*, ch. 3 (dungeons) & ch. 5 (grammars /
  L-systems); Nelson & Smith, ch. 8 (ASP). `pcgbook_chapter03` / `pcgbook_chapter05` / `pcgbook_chapter08`
- de Kegel/Furtado-style weighted graph-grammar pipeline (parallel-programming puzzles). `10.1145/3102071.3102079`
- Viana & Dos Santos (2021). *Procedural Dungeon Generation: A Survey* (Font et al. two-step). `10.5753/jis.2021.999`
- Cooper (2022). *Sturgeon: Tile-Based PLG via Learned and Designed Constraints.* `10.1609/aiide.v18i1.21944`
- Raistrick et al. (2024). *Infinigen Indoors.* `10.48550/arXiv.2406.11824`
- Tutenel et al. (2010). *A Semantic Scene Description Language for Procedural Layout Solving.* `10.1609/aiide.v6i1.12398`
- Gravina, Khalifa, Liapis, Togelius, Yannakakis (2019). *PCG through Quality Diversity.* `10.1109/CIG.2019.8848053`
- Mouret & Clune (2015). *Illuminating Search Spaces by Mapping Elites.* `10.48550/arXiv.1504.04909`
- Alvarez, Dahlskog, Font, Togelius (2020). *Interactive Constrained MAP-Elites.* `10.48550/arXiv.2003.03377`
- Chakraborttii, Ferreira, Whitehead (2017). *Towards Generative Emotions … Surprise (VCL).* `10.1145/3102071.3110577`
- Lume — storylet procedural narrative (2019). `10.1145/3337722.3337759`
- Riedl (2010). *A Comparison of Interactive Narrative System Approaches …* `10.1145/1822309.1822325`
- Smith & Whitehead (2010). *Analyzing the Expressive Range of a Level Generator.* `10.1145/1814256.1814260`
- Baek et al. (2025). *PCGRLLM: LLM-Driven Reward Design for PCGRL.* `10.48550/arXiv.2502.10906`
- Gallotta et al. (2024). *Large Language Models and Games: A Survey and Roadmap.* `10.48550/arXiv.2402.18659`
- In-repo precedent: Karth & Smith (2017), *WFC is Constraint Solving in the Wild* (`ir.rs` header); Wang et
  al. POET, `arXiv:1901.01753` (`world_genome.rs`).
