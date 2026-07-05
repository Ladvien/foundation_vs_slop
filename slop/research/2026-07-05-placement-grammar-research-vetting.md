# Placement Grammar — Research Vetting & Extensible Implementation Plan

Deep vetting of the placement-rule-grammar plan against current SOTA (foundations **and**
engineering), redesigned for three extensibility axes (pluggable solver backends weighted highest),
with an agent-executable roadmap and a cataloged primary-source bibliography.

Audience: implementing agent + engineer. Companion docs: `2026-07-05-placement-rule-grammar.md`
(the grammar), `2026-07-05-placement-grammar-implementation.md` (the codebase-specific plan).

---

## 0. Verdict (bottom line up front)

The foundations are **sound and correctly chosen for the actual constraints** — a bespoke asset kit,
no training data, and a need for determinism, controllability, and small compute. The field has moved
to learned/diffusion/LLM scene synthesis (ATISS, DiffuScene, Holodeck, LayoutGPT), but those buy
plausibility at the cost of a large dataset dependency (3D-FRONT/Structured3D) and loss of exact
control — the wrong trade for this project. Notably, the strongest *current* pure-procedural system,
**Infinigen Indoors (CVPR 2024)**, reaches photorealistic SOTA with **no learning at all** — direct
validation that staying classical is defensible, not dated.

Three things must change:

1. **Generalize the two hardcoded engines behind a `Solver` trait from the start.** The two-backend
   split is right *as an instance*, wrong *as a ceiling*. This is the highest-priority refactor.
2. **Add a global/cardinality backend to the roadmap.** Cardinality ("one door per room") and
   long-range hard constraints ("TV faces sofa across the room") fall into a gap between grid-WFC
   (local only) and MCMC (soft only). This is the one real architectural hole.
3. **Fix the panic path and make roles/groups extensible.** WFC is NP-hard; contradictions must
   degrade to partial placement, never `panic!` (the project mandates this anyway).

Everything else in the plan — the grammar decomposition, manifest-as-adapter, engine-free generation,
RON data, region-conditioning over hardcoded room types — is validated and should stay.

---

## 1. Foundations vetted against SOTA

### 1.1 WFC / Model Synthesis (the hard+local engine) — **SOUND, current**
- Karth & Smith formally established that **WFC *is* constraint solving** (a greedy, non-backtracking
  finite-domain CSP), which grounds it academically rather than treating it as a folk algorithm. Their
  later journal work frames it explicitly as constraint solving + ML. This is the theoretical license
  for the whole "grammar compiles to a constraint problem" architecture.
- Merrell's Model Synthesis (2007→2011) is the same core with a richer constraint taxonomy
  (adjacency, dimensional, algebraic, incidence, connectivity, large-scale) and is provably reducible
  to context-sensitive grammars (TVCG 2011 §7.3). WFC is a rediscovery/special case.
- **Limitation confirmed:** deciding whether a tiling admits a non-failing solution is NP-hard;
  vanilla WFC is greedy and can hit contradictions. Mitigations are well-known: backtracking
  (Newgas/DeBroglie), constraint relaxation, hierarchical/nested WFC, and design-level constraint
  extensions (Sandhu et al., FDG 2019). Graph-WFC (Kim et al.) removes the grid assumption — relevant
  for non-grid furniture relations.
- **Verdict:** keep as the default hard+local backend. It is the correct classical choice and is still
  actively researched (2019–2024 corpus). Adopt backtracking on the *dense* furniture-tiling pass.

### 1.2 MCMC layout (the soft+relational engine) — **SOUND baseline, now one option among many**
- Merrell et al. (TOG 2011) and the parallel Yu et al. "Make It Home" (TOG 2011) established
  optimization-of-a-cost-function-via-MCMC as the classical furniture-layout method. Fisher et al.
  (SIGGRAPH Asia 2012) added example-based priors. Qi et al. (CVPR 2018) added human-centric
  *stochastic grammar* + affordances — a bridge between grammar and layout worth studying.
- The field then went deep/learned: convolutional priors (Wang 2018), fast conv-generative (Ritchie
  2019), relation-graph planning (PlanIT 2019), recursive VAEs (GRAINS 2019), autoregressive
  transformers (ATISS 2021), diffusion (DiffuScene 2024, LEGO-Net 2023), and LLM-driven
  (LayoutGPT 2023, Holodeck 2024, I-Design, InstructScene 2024).
- **Why classical still wins *here*:** every learned method above needs a large annotated corpus
  (SUNCG — now legally encumbered; 3D-FRONT; Structured3D) and a semantic label space that matches
  the dataset, not your asset kit. They trade determinism and exact rule-compliance for learned
  plausibility. For a game with a hand-authored kit, no dataset, and a need for reproducible,
  rule-controllable output, MCMC-over-declared-cost is the right default.
- **Verdict:** keep MCMC as the default soft backend, but do not privilege it in the architecture —
  it should be *a* `Solver`, swappable for a learned/LLM one if a dataset or budget ever appears.

### 1.3 Alternative foundation considered — **Ritchie's probabilistic-programming line**
Ritchie's Stochastically-Ordered SMC (2015), Neurally-Guided Procedural Models (2016), and
example-based procedural authoring (CGF 2018) treat generation as inference over a probabilistic
program. Powerful and more general than a fixed cost function, but heavier to author and to make
deterministic. **Recommendation:** not the v1 foundation; a valid future `Solver` if you want learned
guidance without a full scene-synthesis dataset.

---

## 2. Highest-priority change — the pluggable `Solver` backend interface

The plan hardcodes exactly two engines. Karth & Smith's result ("placement is constraint solving")
says the grammar should compile to a **solver-agnostic constraint problem** that any capable backend
can consume. Make that explicit:

```rust
/// Grammar IR compiled for one bounded region. Engine-free (no Bevy types).
pub struct PlacementProblem<'a> {
    pub region: &'a Region,          // boundary polygon, openings, property bag, adjacency
    pub candidates: Vec<Candidate>,  // asset instances + their placement degrees of freedom
    pub constraints: Vec<Constraint>,// compiled rules: scope + predicate + modality + guard
}

pub enum Outcome {
    Assignment(Vec<Placement>),               // hard solve: one consistent assignment
    Ranked(Vec<(f64, Vec<Placement>)>),       // soft solve: cost-ranked samples
    Partial { placed: Vec<Placement>, unsatisfied: Vec<ConstraintId> }, // graceful degradation
}

pub struct Capabilities {
    pub hardness: Hardness,      // Hard | Soft | Both
    pub locality: Locality,      // Local | Relational | Global
    pub cardinality: bool,       // can enforce count(...) constraints
    pub deterministic: bool,     // reproducible under a seed
    pub needs_training_data: bool,
}

pub trait Solver: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    fn solve(&self, p: &PlacementProblem, rng: &mut Rng) -> Result<Outcome, SolveError>;
}
```

**Orchestrator:** partition the compiled constraints by region/scope into groups; compute each group's
required `Capabilities`; route the group to the first registered `Solver` that covers it. Solvers live
in a `Vec<Box<dyn Solver>>` (or a Bevy resource). The two current engines become the first two impls:

| Solver | Capabilities | Basis |
|---|---|---|
| `WfcSolver` (wrap existing `wfc.rs`) | Hard, Local | Gumin 2016; Merrell 2011; Karth & Smith 2017 |
| `MetropolisSolver` (new) | Soft, Relational | Merrell 2011; Yu 2011 |
| `ConstraintSolver` / `AspSolver` (Stage 4) | Hard, Global, Cardinality | Smith & Mateas 2011; Karth & Smith |
| `LearnedSolver` / `LlmSolver` (future seam) | Both, needs_training_data | ATISS/DiffuScene/LayoutGPT |

This single abstraction *is* extensibility axis #1, and it is what lets axes #2 (assets) and #3
(domains) compose cleanly, because every backend consumes the same IR.

---

## 3. The three extensibility axes

### 3.1 Pluggable backends (highest priority) — §2 above.

### 3.2 Asset-library swapping — **validated; strengthen the manifest to affordances**
Manifest-as-adapter is the right pattern. Make it more portable by annotating **affordances**, not
just categories, so rules can target *what an object affords* (sit / sleep / store / support / emit /
occlude) rather than kit-specific names. This is how affordance- and activity-based scene synthesis
generalizes across object sets (Fisher 2012; Qi 2018). Minimum schema:

```ron
( glb: "<path>", category: "<opaque>", tags: ["<role>", "<opaque>..."],
  footprint: (w, d), front: <Anchor>, support_surfaces: [<Rect>], attach_points: [<Anchor>],
  affordances: ["sit","support"], clearance: [(Side, m)] )
```
Roles (`Anchor{host}`, `Tiled`, `Freestanding`, `Scatter{surface}`) stay the dispatch key; affordances
and categories are opaque tokens the code matches but never interprets. Porting = author one manifest.

### 3.3 Domain swapping (interiors → urban / dungeon) — **academically supported**
Shape grammars (Stiny 1972), split grammars (Wonka, *Instant Architecture* 2003), and CGA shape
(Müller, *Procedural Modeling of Buildings* 2006) show the placement/subdivision formalism generalizes
to architecture and cities; Parish & Müller (2001) is the CityEngine root. The grammar's
scope/predicate/modality decomposition is domain-agnostic — what changes is (a) the manifest roles and
(b) the `Region` primitive. Make `Region` a generic **bounded container**: boundary polygon + openings
+ property bag + adjacency edges. Interiors=rooms, urban=parcels/lots, dungeon=cells. New domains add
predicates (e.g., `aligned(a, road)`), not new engines. The codebase already runs WFC at dungeon scale,
so in-repo domain range is proven.

---

## 4. Engineering validation (Bevy 0.19 / Avian3D / Rust)

- **Generation ↔ runtime boundary:** keep *all* solve logic engine-free (pure Rust, zero Bevy types),
  exactly as `wfc.rs` already is; a thin `furnish` Bevy system consumes `Outcome` and spawns entities.
  This is the correct ECS discipline and is already established in the repo — extend it, don't break it.
- **Determinism:** thread one seeded RNG stream through orchestrator → solvers. The repo's xorshift64
  `Rng` is adequate but consider `rand` + `rand_chacha::ChaCha8Rng` for a portable, well-tested,
  reproducible stream; use **`bevy_rand`** for any ECS-side randomness (Bevy system execution order is
  nondeterministic, so RNG must not depend on it). Derive per-region sub-seeds (splitmix/`seed ^ hash(region_id)`)
  so regions solve independently and reproducibly.
- **No panics (mandated by `CLAUDE.md`):** `Solver::solve` returns `Result`; contradictions/timeouts
  return `Outcome::Partial` (best-effort), never `panic!`. Fix `wfc.rs`'s panic-on-non-convergence on
  the placement path — bounded restarts, then degrade. (The room-graph pass rarely fails on its
  permissive alphabet; the dense furniture-tiling pass is where this bites.)
- **Performance:** MCMC here is *offline* and *per-region independent* → embarrassingly parallel
  (`rayon`); a few thousand Metropolis iterations on a small room is sub-millisecond-to-millisecond. The
  paper needed GPU only for interactive sub-second re-suggestion — not required for one-shot generation.
  Budget iterations per region; early-stop on cost plateau (paper: <0.1% improvement past ~10k iters).
- **Data / hot-reload:** rules + manifest as RON assets (`serde` is already a dep; `ai_tuning.ron` is
  precedent). Bevy `AssetServer` hot-reload can re-trigger generation in dev. Keep the grammar IR
  `Serialize`/`Deserialize`.

---

## 5. Risks & gaps (sharpened, with mitigations and sources)

| # | Risk | Severity | Mitigation | Grounding |
|---|---|---|---|---|
| R1 | Hand-tuned MCMC weights are a maintainability risk | Low–Med | Weights in RON per-profile; Merrell 2011 found weights robust to 2× perturbation; future `LearnedSolver` can fit weights via inverse optimization | Merrell 2011; O'Donovan 2014 (learning layouts); Fisher 2012 |
| R2 | **Cardinality + long-range hard constraints fall into a gap** between local WFC and soft MCMC ("one door/room"; "TV faces sofa across room") | **High** | Add a global/cardinality `Solver` (ASP/CSP) for counts & global rules; use graph-WFC or high-weight soft terms for long-range relations. This is the main reason the `Solver` trait must exist | Smith & Mateas 2011 (ASP for PCG); Karth & Smith 2017/2022; Kim 2020 (graph-WFC) |
| R3 | WFC NP-hardness / contradiction → current impl panics | Med | Backtracking (DeBroglie), constraint relaxation, bounded restarts → `Partial` | Merrell 2011 (NP-hardness note); Newgas/DeBroglie; Sandhu 2019 |
| R4 | "Four roles" abstraction leaks on stacking/support chains, articulated groups (dining set as a unit), boundary-spanning footprints, forbidden zones | Med | Make roles an **open** set (trait/open enum); model **groups as first-class scopes** (the paper's `G`), not roles | Merrell 2011 (groups); Qi 2018 (activity groups) |
| R5 | Room typing quality & cross-room constraints | Med | Region-conditioning is sound; add a **region adjacency graph** as first-class so cross-room rules and typing policies can use it | Qi 2018; PlanIT 2019 (relation graphs) |

---

## 6. Agent-executable roadmap (Stages 0–5)

Each stage lists the deliverable, the acceptance criterion (**AC**), and grounding sources. The
`Solver` trait is introduced at Stage 1 and every subsequent engine is an impl of it — no bolt-on.

**Stage 0 — Region plumbing + IR skeleton.**
Change `src/dungeon.rs::generate()` to retain `Vec<Region>` on the `Dungeon` resource
(`rect`, `openings` from coarse `CellData.open`, `adjacency`, `property bag`). Define the grammar IR
(`Scope`/`Predicate`/`Modality`/`Constraint`) and the `PlacementProblem`/`Outcome`/`Capabilities`/
`Solver` types (§2). No solving; spawn one debug cube per region.
**AC:** regions are addressable and adjacency-linked; IR + trait compile; debug cubes render (screenshot).

**Stage 1 — `Solver` trait + first backend (adapt existing WFC).**
Wrap `wfc.rs` as `WfcSolver: Solver { Hard, Local }`. Build the orchestrator: partition constraints
→ capability profile → route. Regenerate the room graph *through* the `Solver` interface.
**AC:** dungeon still generates via the trait; capability-based dispatch selects `WfcSolver` for
local-hard groups. Grounding: Gumin 2016; Merrell 2011; Karth & Smith 2017.

**Stage 2 — Manifest adapter + anchor pass + ECS spawn.**
Affordance-annotated RON manifest (§3.2); FBX→GLB for the chosen kit (per-set adapter cost). Implement
deterministic `Anchor`-role placement (lights→ceiling, doors→openings, curtains→windows) as a trivial
solver or direct pass. Spawn via `WorldAssetRoot(load(GltfAssetLabel::Scene(0)…))` + Avian collider,
mirroring `src/squad.rs`/`src/nest.rs`.
**AC:** first furnished frame; changing the manifest changes output with no code diff.

**Stage 3 — `MetropolisSolver` (soft/relational).**
Port Merrell 2011 §2–3: cost terms (clearance, pairwise distance/angle, wall-alignment, balance,
emphasis/symmetry) + Metropolis–Hastings; per-region, seeded, `rayon`-parallel, weights in RON,
`Partial` on failure. Cite the paper in-code per `CLAUDE.md`.
**AC:** rooms look designed; term ablation reproduces the paper's Fig. 6 behavior. Grounding: Merrell
2011; Yu 2011.

**Stage 4 — Global/cardinality backend + long-range relations.**
Add `ConstraintSolver: Solver { Hard, Global, Cardinality }` (ASP-style or a small custom finite-domain
solver — the repo already ships a GameAIPro finite-domain-solver reference) for counts/global rules;
optionally graph-WFC for relational placement. Wire long-range "facing" as either a high-weight soft
term or a graph edge.
**AC:** "exactly one door per room" and one long-range facing constraint are satisfied deterministically.
Grounding: Smith & Mateas 2011; Karth & Smith; Kim 2020; GameAIPro Ch.26.

**Stage 5 — Extensibility acceptance tests (the falsifiable checks).**
- **Backend swap:** register an alternate `Solver` for a rule group; grammar unchanged.
- **Asset swap:** repoint the manifest at the kenney kit already in `assets/`; zero code diff.
- **Domain swap:** instantiate `Region` for an exterior/parcel layout with one new predicate
  (`aligned(a, road)`); reuse the same orchestrator + grammar.
**AC:** all three pass → the three axes hold. This is the acceptance test for the whole architecture.

**Future seam (not scheduled):** a `LearnedSolver`/`LlmSolver` implementing the same trait. Not needed
now (no dataset, determinism required), but Infinigen Indoors (2024) shows pure-procedural reaches SOTA
quality, and the trait makes a learned backend a drop-in if a dataset or LLM budget appears.

---

## 7. Keep / change / future-proof

**Keep:** grammar decomposition (scope/predicate/modality); the two engines *as the first two Solvers*;
manifest-as-adapter; engine-free generation; RON data; region-conditioning over hardcoded room types.

**Change:** (1) generalize both engines behind the `Solver` trait from Stage 1; (2) open roles +
first-class groups; (3) schedule a global/cardinality backend (R2); (4) add a region adjacency graph;
(5) `panic!` → `Partial`; (6) affordance-annotate the manifest; (7) generalize `Region` for domains.

**Future-proof:** leave the `LearnedSolver`/`LlmSolver` seam; keep the IR serializable so a solver can
be an external process; consider `rand_chacha` for portable determinism.

---

## 8. Catalog of primary sources

Organized by topic. `[home-still: <stem>]` = already in the local corpus. `[external]` = recommend
download. Classification: **F** foundational · **S** current SOTA · **A** alternative-to-consider.
DOIs shown were verified this session; entries with venue/year only should have identifiers confirmed
before formal citation.

### A. Constraint-based generation — Model Synthesis / WFC (the hard+local core)
1. **F** Merrell, P. "Example-Based Model Synthesis." *I3D* 2007. DOI 10.1145/1230100.1230119. `[home-still: model_synthesis]` — origin of model synthesis; the adjacency constraint = WFC's core.
2. **F** Merrell, P., Manocha, D. "Continuous Model Synthesis." *ACM TOG (SIGGRAPH Asia)* 2008. DOI 10.1145/1409060.1409111. `[home-still: continuous]` — non-axis-aligned extension.
3. **F** Merrell, P., Manocha, D. "Constraint-Based Model Synthesis." *SPM* 2009. DOI 10.1145/1629255.1629269. `[external]` — dimensional/algebraic/connectivity constraints (preliminary to #4).
4. **F** Merrell, P., Manocha, D. "Model Synthesis: A General Procedural Modeling Algorithm." *IEEE TVCG* 17(6), 2011. DOI 10.1109/TVCG.2010.112. `[home-still: 10.1109_tvcg.2010.112]` — full constraint taxonomy + proof of equivalence to context-sensitive grammars.
5. **F** Merrell, P. "Model Synthesis." PhD dissertation, UNC Chapel Hill, 2009. `[home-still: merrell09]` — comprehensive treatment.
6. **A** Merrell, P. "Example-Based Procedural Modeling Using Graph Grammars." *ACM TOG* 2023. DOI 10.1145/3592119. `[external]` — non-grid, tile-free successor; relevant to non-grid relational placement.
7. **F** Gumin, M. "Wave Function Collapse." Software, 2016. github.com/mxgmn/WaveFunctionCollapse. `[external]` — the popularization; overlapping + simple-tiled models.
8. **F** Karth, I., Smith, A. M. "WaveFunctionCollapse is Constraint Solving in the Wild." *FDG* 2017. `[external]` — establishes WFC = finite-domain CSP; the theoretical basis for the pluggable-solver architecture.
9. **S** Karth, I., Smith, A. M. "WaveFunctionCollapse: Content Generation via Constraint Solving and Machine Learning." *IEEE Transactions on Games*, 2022. `[external]` — journal extension; constraint-solving + learning framing.
10. **A** Sandhu, A., Chen, Z., McCoy, J. "Enhancing Wave Function Collapse with Design-Level Constraints." *FDG* 2019, Art. 17. DOI 10.1145/3337722.3337752. `[home-still: 10.1145_3337722.3337752]` — non-local, weight-recalculation, area-propagation constraint classes.
11. **A** Kim, H., Hahn, T., Kim, S., Kang, S. "Graph-Based Wave Function Collapse Algorithm for PCG in Games." *IEICE Trans. Inf. & Syst.*, 2020. DOI 10.1587/transinf.2019edp7295. `[home-still: 10.1587_transinf.2019edp7295]` — drops the grid; graph adjacency for relational placement.
12. **A** Newgas, A. ("BorisTheBrave"). "DeBroglie" WFC library + technical writing on propagation & backtracking. github.com/BorisTheBrave/DeBroglie. `[external]` — production-grade WFC with backtracking/constraints; the practical reference for R3.

### B. Optimization-based scene layout (the soft+relational core)
13. **F** Merrell, P., Schkufza, E., Li, Z., Agrawala, M., Koltun, V. "Interactive Furniture Layout Using Interior Design Guidelines." *ACM TOG (SIGGRAPH)* 30(4), 2011. DOI 10.1145/2010324.1964982. `[home-still: furnitureLayout2]` — the density-function + Metropolis–Hastings formulation being ported.
14. **F** Yu, L.-F., Yeung, S.-K., Tang, C.-K., Terzopoulos, D., Chan, T. F., Osher, S. "Make It Home: Automatic Optimization of Furniture Arrangement." *ACM TOG (SIGGRAPH)* 30(4), 2011. `[external]` — parallel seminal MCMC-layout paper; alternative cost terms.
15. **A** Fisher, M., Ritchie, D., Savva, M., Funkhouser, T., Hanrahan, P. "Example-Based Synthesis of 3D Object Arrangements." *ACM TOG (SIGGRAPH Asia)* 31(6), 2012. `[external]` — example priors for arrangements; path to learning MCMC weights.
16. **A** O'Donovan, P., Agarwala, A., Hertzmann, A. "Learning Layouts for Single-Page Graphic Designs." *IEEE TVCG* 20(8), 2014. DOI 10.1109/TVCG.2014.48. `[external]` — inverse-optimization of layout weights; mitigation for R1.

### C. Learned / deep / diffusion / LLM scene synthesis (SOTA — future backends, dataset-bound)
17. **S** Qi, S., Zhu, Y., Huang, S., Jiang, C., Zhu, S.-C. "Human-Centric Indoor Scene Synthesis Using Stochastic Grammar." *CVPR* 2018. `[external]` — grammar + affordances + MCMC; the closest learned/grammar hybrid to this design.
18. **S** Wang, K., Savva, M., Chang, A. X., Ritchie, D. "Deep Convolutional Priors for Indoor Scene Synthesis." *ACM TOG (SIGGRAPH)* 37(4), 2018. `[external]`.
19. **S** Ritchie, D., Wang, K., Lin, Y.-A. "Fast and Flexible Indoor Scene Synthesis via Deep Convolutional Generative Models." *CVPR* 2019. `[external]`.
20. **S** Wang, K., Lin, Y.-A., Weissmann, B., Savva, M., Chang, A. X., Ritchie, D. "PlanIT: Planning and Instantiating Indoor Scenes with Relation Graph and Spatial Prior Networks." *ACM TOG (SIGGRAPH)* 38(4), 2019. `[external]` — explicit relation graphs (relevant to R5).
21. **S** Li, M., Patil, A. G., Xu, K., et al. "GRAINS: Generative Recursive Autoencoders for Indoor Scenes." *ACM TOG* 38(2), 2019. `[external]`.
22. **S** Paschalidou, D., Kar, A., Shugrina, M., Kreis, K., Geiger, A., Fidler, S. "ATISS: Autoregressive Transformers for Indoor Scene Synthesis." *NeurIPS* 2021. `[external]` — strong autoregressive baseline.
23. **S** Wei, Q. A., et al. "LEGO-Net: Learning Regular Rearrangements of Objects in Rooms." *CVPR* 2023. `[external]`.
24. **S** Tang, J., et al. "DiffuScene: Denoising Diffusion Models for Generative Indoor Scene Synthesis." *CVPR* 2024. `[external]` — diffusion SOTA.
25. **S** Feng, W., et al. "LayoutGPT: Compositional Visual Planning and Generation with Large Language Models." *NeurIPS* 2023. `[external]` — LLM-as-layout-planner.
26. **S** Yang, Y., et al. "Holodeck: Language-Guided Generation of 3D Embodied AI Environments." *CVPR* 2024. `[external]` — LLM + constraint solver for full scenes; a hybrid worth studying for the ASP backend.
27. **S** Lin, C., Mu, Y. "InstructScene: Instruction-Driven 3D Indoor Scene Synthesis with Semantic Graph Prior." *ICLR* 2024. `[external]`.
28. **S / A** Raistrick, A., et al. "Infinigen Indoors: Photorealistic Indoor Scenes using Procedural Generation." *CVPR* 2024. `[external]` — **pure procedural, no learning, controllable**; the key evidence that the classical direction is not dated.

### D. Datasets (only needed if a learned backend is ever pursued)
29. Fu, H., et al. "3D-FRONT: 3D Furnished Rooms with Layouts and Semantics." *ICCV* 2021. `[external]`.
30. Zheng, J., et al. "Structured3D: A Large Photo-realistic Dataset for Structured 3D Modeling." *ECCV* 2020. `[external]`.
31. Song, S., et al. "Semantic Scene Completion from a Single Depth Image" (SUNCG). *CVPR* 2017. `[external]` — historically dominant; **now legally encumbered**, avoid.

### E. Shape grammars & architectural/urban PCG (domain-swap axis)
32. **F** Stiny, G., Gips, J. "Shape Grammars and the Generative Specification of Painting and Sculpture." *IFIP Congress* 1972. `[external]` — foundational shape grammar.
33. **F** Wonka, P., Wimmer, M., Sillion, F., Ribarsky, W. "Instant Architecture." *ACM TOG (SIGGRAPH)* 22(3), 2003. `[external]` — split grammars.
34. **F** Müller, P., Wonka, P., Haegler, S., Ulmer, A., Van Gool, L. "Procedural Modeling of Buildings." *ACM TOG (SIGGRAPH)* 25(3), 2006. `[external]` — CGA shape; CityEngine's basis.
35. **F** Parish, Y. I. H., Müller, P. "Procedural Modeling of Cities." *SIGGRAPH* 2001. `[external]` — urban-scale placement roots.
36. **A** Fukaya, K., Daylamani-Zad, D., Agius, H. "Intelligent Generation of Graphical Game Assets: A Systematic Review." *ACM Computing Surveys*, 2025. DOI 10.1145/3708499. `[home-still: 10.1145_3708499]` — the "object placement" taxonomy underpinning the role model.
37. **A** Kutzias, D., von Mammen, S. "Recent Advances in Procedural Generation of Buildings: From Diversity to Integration." *IEEE Transactions on Games*, 2023. DOI 10.1109/tg.2023.3262507. `[home-still: 10.1109_tg.2023.3262507]` — building/interior generation survey; catalogs interior-placement systems.

### F. Constraint-solving formulations & practitioner PCG (pluggable-backend axis)
38. **F** Smith, A. M., Mateas, M. "Answer Set Programming for Procedural Content Generation: A Design Space Approach." *IEEE TCIAIG* 3(3), 2011. `[external]` — ASP for PCG; the basis for the global/cardinality backend (R2).
39. **F** Shaker, N., Togelius, J., Nelson, M. J. "Procedural Content Generation in Games." Springer, 2016. www.pcgbook.com (free). `[external]` — the field's standard reference; constraint-satisfaction & search-based chapters.
40. **A** "Rolling Your Own Finite-Domain Constraint Solver." *Game AI Pro 2*, Ch. 26. `[home-still: GameAIPro2_Chapter26_...]` — practical finite-domain solver; a template for the Stage-4 `ConstraintSolver`.
41. **A** "Procedural Content Generation: An Overview." *Game AI Pro 2*, Ch. 40. `[home-still: GameAIPro2_Chapter40_...]` — practitioner framing.
42. **A** Ritchie, D., Mildenhall, B., Goodman, N. D., Hanrahan, P. "Controlling Procedural Modeling Programs with Stochastically-Ordered Sequential Monte Carlo." *ACM TOG (SIGGRAPH)* 34(4), 2015. `[external]`; and Ritchie, Thomas, Hanrahan, Goodman, "Neurally-Guided Procedural Models." *NeurIPS* 2016. `[external]` — probabilistic-programming alternative foundation (§1.3).
43. **A** Ritchie, D., Jobalia, S., Thomas, A. "Example-Based Authoring of Procedural Modeling Programs with Structural and Continuous Variability." *Computer Graphics Forum (Eurographics)* 37(2), 2018. DOI 10.1111/cgf.13371. `[external]`.

### G. Engineering (tooling, not papers)
- **Bevy 0.19** ECS (docs.rs/bevy) — generation-time vs runtime boundary; system ordering nondeterminism.
- **`bevy_rand`** + **`rand_chacha`** (ChaCha8Rng) — reproducible, portable PRNG in ECS.
- **Avian3D 0.7** — colliders for spawned furniture.
- **RON** (`ron` crate) — rule/manifest serialization + hot-reload.

---

*Verification note:* DOIs on entries 1–5, 6, 10, 11, 13, 16, 36, 37, 43 were checked against
Crossref/OpenAlex this session. SOTA entries (C/D) and grammar entries (E) list venue/year from
established record; confirm exact DOIs/arXiv IDs before formal citation. None of the learned-SOTA
papers (C) are currently in the home-still corpus — pull the shortlist (17, 22, 24, 26, 28, 38) first
if you want the pluggable-backend and global-constraint design fully grounded in-library.
