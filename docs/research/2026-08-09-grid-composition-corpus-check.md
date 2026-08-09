# Grid composition — what the corpus actually says

**Date:** 2026-08-09
**Question:** the lattice-composition design (compose tab, 3×3 of 3×3, tile = floor + wall + sconce) —
does home-still support it, and does it answer §5's blocking question?
**Method:** `distill_search` over the local corpus (5 queries), `catalog_read` + `markdown_read` to
confirm two identifications. Companion docs: `2026-07-24-placement-rule-unification.md`,
`2026-08-08-kitbashing-guidance.md`, `2026-07-05-placement-grammar-research-vetting.md`.

---

## 0. Verdict

The corpus supports the lattice direction, but it materially changes two of the four open questions and
adds a warning about a third.

| Open question | Corpus verdict |
|---|---|
| 1. Mount or layer index sets Y? | **Neither — and the conflict is a false one.** CGA types split values as absolute vs relative. `1.8` never becomes `5.4 of 9`. |
| 2. One 9×9 lattice or two levels? | **Two levels, but the boundary is dimension, not cell count.** CGA's component split reduces 3D → 2D face → 1D edge. Edge tokens read off the component. |
| 3. Replace free placement? | **No — live beside it.** Unanimous; already the conclusion of `2026-07-24`. |
| 4. Nest at authoring or compose at stamp? | **Both, doing different jobs.** Karth & Smith and Sturgeon both give a way to avoid the cross-product without authoring composites. |
| §5. Floor: one plane or per cell? | **Per-cell ownership + a global plane registry for alignment.** The corpus splits, and the split is the answer. |

One correction to a prior doc, and one high-value download, in §6.

---

## 1. The vertical friction dissolves — CGA types the values

`10.1145_1179352.1141931` — Müller, Wonka, Haegler, Ulmer & Van Gool (2006), *Procedural Modeling of
Buildings* (CGA Shape). This is the most load-bearing find in the sweep.

The split rule:

```
1: floor ~ Subdiv("X", 2, 1r, 1r, 2) { B | A | A | B }
1: floor ~ Repeat("X", 2) { B }
```

> "not all architectural parts scale equally well, and it is important to have the possibility to
> distinguish between **absolute values (values that do not scale) and relative values (values that do
> scale)**. Values are considered absolute by default and we will use the letter `r` to denote relative
> values"

with relative values substituted as `r_i * (Scope.sx - Σabs_i) / Σr_i`.

**So the sconce is `Subdiv("Y", 1.8, 1r)`.** `1.8` stays `1.8` on a 3 m tile and on a 4 m tile. The
layer index is *derived from the split*, never authored. There is no conversion step, so there is no
"5.4 of 9" to round.

This kills the framing of question 1. It is not *mount vs layer index* — those are the same mechanism
with a type tag. `OnWall { height: 1.8 }` becomes an absolute split value; "third band up" becomes a
relative one. One mechanism, two value types, and the type is declared where the number is written.

`Repeat` matters too: the repeat count is `⌈Scope.sx / tile⌉` with element size adjusted to fit. That is
the correct primitive for a run of pipe or a row of panels, and it is *not* a lattice of fixed cells.

---

## 2. The second level is a dimension reduction, not a finer grid

Same paper, the **component split**:

```
1: a → Comp(type, param) { A | B | ... | Z }
Comp("faces") {A}   Comp("sidefaces") {B}   Comp("edges") {B}   Comp("edge", 3) {A}
```

> "Up until this point all shapes (scopes) have been three-dimensional. The following command allows to
> split into shapes of lesser dimensions"

This is the real two-level structure, and it is better than "coarse 3×3 / fine 3×3":

- The **3D scope** is the tile volume — where meshes get seated.
- The **2D component** is a face — where edge/interface tokens belong.
- The **1D component** is an edge — where corner agreement belongs.

Your `summarise_face` complaint (a 2.4 m wall at shipped divisions is ten cells all saying the same
word, and worse at 9) is the symptom of reading a token off a *cell count* instead of off a
*component*. A face is one component regardless of how finely its interior is subdivided. Read tokens
there and the divisions stop leaking into the adjacency vocabulary.

### The warning attached to this, from the same authors

> "As split grammars maintain a strict hierarchy, modeling is fairly simple, but also limited. However,
> after introducing rules for combinations of shapes and more general volumetric shapes such as roofs,
> **the strict hierarchy of the split-grammar can no longer be enforced.** We can confirm, that the idea
> of split rules is a very suitable primitive to generate façade details, but we did not find it
> suitable for many forms of mass modeling."

Read that as: **the lattice is right inside a tile and wrong as the map's mechanism.** The design already
splits those (map = grid of tile references; lattice = inside a tile), and this is the citation that
says the split is not optional — CGA had to learn it after building the strict-hierarchy version.

### And a cost argument against a uniform 9×9

`merrell09` / `10.1109_tvcg.2010.112` — Merrell & Manocha, on model synthesis' limits:

> "it is difficult to generate both large and small objects simultaneously. Small objects require
> closely spaced planes while large objects require large volumes which together means that **many
> planes must be created**."

Uniform fine subdivision to seat a sconce forces that density across the whole problem. Two levels with
different jobs is not just tidier; it is what keeps the constraint problem small.

---

## 3. The "four meshes in one grid unit" bug has a named precedent and a named fix

CGA hit exactly this, and it is their Figure 2 motivation:

> "Existing methods of procedural architecture can either place shaders on the individual volumes or use
> split rules for procedural refinement. In both cases several unwanted intersections will cut windows
> (or other elements) in unnatural ways, **as the volumes are not aware of each other**."

Their fix was **not** a stronger hierarchy. It was an occlusion query with explicit scoping:

- `Shape.occ("all") == "none" ~> door` — variant selection is a *query at derivation time*, not a stored
  variant.
- `Shape.occ("active")` — test only against shapes not yet replaced.
- `Shape.occ("balcony")` — test against a labelled subset.
- `Shape.occ("noparent")` — **"one of the most important subsets contains all shapes in the derivation
  tree except the current shape's predecessors. With this subset, we avoid the querying of parent
  shapes, which, in the case of a split, always occlude their successor shapes."**
- `Shape.occ("noparent", "distance", 4)` — dilate before testing (this is a clearance test).
- Backed by an octree for the spatial queries.

That fourth bullet is the one to take: if the overlap test runs inside a composition, **the tile's own
envelope must be excluded as an occluder of its own members**, or every composite reports a fault
against itself. Worth checking against whatever `composition::interface` does today when it resolves
members against the envelope-as-map.

---

## 4. §5 — floor: one plane or one feature per cell?

The corpus genuinely splits, and I think the split *is* the answer: they are answering two different
questions.

**Per-cell — `10.1609_aiide.v6i1.12398`, Tutenel, Smelik, Bidarra & de Kraker (2010).** The paper
`2026-07-24` already builds Stage 2 on. Objects carry typed **features** — geometric regions with
semantics (`top`, `front`, `storage`, one per shelf on a bookcase). Two feature types carry built-in
layout semantics: *off-limits* features "cannot overlap any other features", and *clearance* features
"can only overlap other clearance features". Relations are stated **between features, not objects**.
Under this model, "this cell is floored by a tile" is a feature the tile carries, and the overlap test
is feature-vs-feature and local — exactly the property §5 wants.

**Per-cell again — `10.48550_arxiv.2406.11824`, Infinigen Indoors.** `StableAgainst(Tag.Bottom,
Tag.Floor)`. The relation is a tag pair and the parent is whichever object carries `Floor`. Not a global
plane.

**One plane — CGA, but as an alignment registry, not an owner.**

> "In the simplest form, all faces of the volumetric shapes of the mass model are stored as **global
> construction planes**. If we are selecting a planar (two-dimensional) scope on the side of a façade as
> the active rule, the scope can be intersected by the global construction planes defining **snap
> lines**."

and what it buys:

> "Note how the **floor levels are automatically aligned over all solids**, e.g. a higher floor was
> forced below the tapering."

Snap lines have two behaviours worth stealing verbatim: for a `Repeat`, the snap line *divides the scope
and the repeat runs per part*; for a `Subdiv`, the snap line *moves only the nearest split and leaves
everything else alone*. Notation is a suffix on the axis: `Repeat("XS", tile_width)`, `Subdiv("YS", ...)`.

**Recommendation: take both.** The tile owns a floor feature (per-cell, so overlap stays local and
"floored by a tile" is directly expressible), and the room keeps a registry of construction planes that
splits snap to (so twenty adjacent tiles don't drift, and a mezzanine or a step doesn't need a per-cell
agreement protocol). CGA needed both and used an octree for the queries. These are not competing
answers to one question; they are answers to *ownership* and *alignment* respectively, and conflating
them is what makes §5 feel blocking.

---

## 5. Avoiding wall × light × pipe × door

Three mechanisms in the corpus, none of which require authoring the cross product.

**a. Adjacency-as-composite — `10.1145_3337722.3341845`, Karth & Smith (2019), footnote 11:**

> "A conceptually simpler way to implement **multi-tile elements** is to give the different parts the
> constraint that the only allowed neighbor in the relevant edge is another part of the multi-tile
> module."

So a composite need not be a stored artefact at all. Author the parts, add "only a part of this module
may sit on that edge", and the solver assembles it. This is the cheapest answer for anything whose
composition is *positional* rather than *artistic*.

**b. Tags as indirection — `10.1609_aiide.v18i1.21944`, Cooper (2022), Sturgeon.**

> "tags, which are labels associated with one or more tiles and can be used to limit what tiles can be
> placed at a location... Sometimes the tile/tag distinction is intentionally blurred: functional tiles
> can be used as tags to constrain image tile placement."

Plus the **functional grid / image grid** separation, generated either simultaneously or sequentially —
"the sequential approach can be more efficient, as reachability does not have to consider what a tile
looks like." A "lit wall" is a wall tile plus a light tag, not a fifth authored variant, and the whole
art set swaps under a fixed functional layout.

**c. Query-time variant selection — CGA `Shape.occ(...) ~> door`, §3 above.**

**So the fork in question 4 is not a fork.** Nest at authoring time when the arrangement is one artistic
decision that should always travel together (a sconce seated in its niche, a pipe run bent around a
corner). Compose at stamp time via tags/adjacency when the combination is a cross product. Different
jobs; both mechanisms should exist.

---

## 6. Free placement stays — no dissent in the corpus

`furnitureLayout2` (Merrell, Schkufza, Li, Agrawala & Koltun 2011) is continuous throughout: clearance
as Minkowski sums, pairwise distance and angle terms, wall alignment, visual balance, MCMC over the
density function. Infinigen is continuous OBBs (`size`, `translation`, `rotation`) under simulated
annealing. `10.1609_aiide.v6i1.12398` places by relation, not by cell. Nothing in the corpus argues for
gridding dressing.

The structure/dressing split proposed in question 3 is the corpus consensus and is already the shape of
`2026-07-24`'s Stage 0–4.

---

## 7. Corrections and gaps

**Correction.** `2026-07-05-placement-grammar-research-vetting.md` lists Karth & Smith (2017),
*WaveFunctionCollapse is Constraint Solving in the Wild*, as entry 8 marked `[external]`, and
`2026-07-24-placement-rule-unification.md` cites it without a corpus stem. **It is in the corpus**:
`10.1145_3102071.3110566`, converted and embedded. Verified by reading page 1 — title, authors, and the
FDG'17 DOI match. Both docs should be updated; the Stage 4 unification argument can cite it from the
library rather than from memory.

**Highest-value download.** Lagae & Dutré (2006), *An Alternative for Wang Tiles: Colored Edges versus
Colored Corners*, ACM TOG 25(4), 1442–1459. It appears in the corpus **only as a citation inside another
paper** (`sig2024_Quad-Optimized_Low-Discrepancy_Sequences`), not as an indexed document. It is the
canonical treatment of exactly the §5 question — whether the constraint lives on the cell edge or on the
corner/vertex (the dual-grid formulation) — and of the tile-count consequences of each. Worth a
`paper_download`.

**Honest gaps.**

- **Nothing in the corpus is about nested tile authoring for game kits.** The nearest hit is HWFC in
  `10.1109_mcg.2024.3447775` (Heese 2024), but that partitions to shrink a quantum circuit, not to
  express semantic nesting — it neither supports nor contradicts the design.
- **The corpus says nothing about the furniture kit or the WFC solvers.** The concern stated up front —
  that the inversion is proved only against the site kit's failure — is untouched by this sweep.
  Everything above argues about *mechanism*, not about whether the existing kits survive it.
- Claims here about `composition::interface`, `stack::resolve_y`, `Envelope::Bounded`, `Placed::at` and
  `summarise_face` are taken from the design conversation, not read from source in this session.

---

## Sources (all read from the local corpus this session)

- Müller, Wonka, Haegler, Ulmer & Van Gool (2006). *Procedural Modeling of Buildings.* SIGGRAPH.
  `10.1145_1179352.1141931` — split/repeat/component rules, absolute vs relative values, occlusion
  queries, snap lines.
- Tutenel, Smelik, Bidarra & de Kraker (2010). *A Semantic Scene Description Language for Procedural
  Layout Solving Problems.* AIIDE. `10.1609/aiide.v6i1.12398` — typed features, off-limits/clearance.
- Cooper (2022). *Sturgeon: Tile-Based Procedural Level Generation via Learned and Designed
  Constraints.* AIIDE. `10.1609/aiide.v18i1.21944` — tags, functional/image grid split.
- Karth & Smith (2019). *Addressing the Fundamental Tension of PCGML with Discriminative Learning.* FDG.
  `10.1145/3337722.3341845` — multi-tile modules via edge constraints (fn. 11).
- Karth & Smith (2017). *WaveFunctionCollapse is Constraint Solving in the Wild.* FDG.
  `10.1145/3102071.3110566` — **present in corpus; prior docs say otherwise.**
- Merrell & Manocha (2010/2011). *Model Synthesis.* IEEE TVCG. `10.1109/tvcg.2010.112`, `merrell09` —
  dimensional/incidence/connectivity constraints; the plane-density limitation.
- Merrell, Schkufza, Li, Agrawala & Koltun (2011). *Interactive Furniture Layout Using Interior Design
  Guidelines.* SIGGRAPH. `furnitureLayout2`.
- Raistrick et al. (2024). *Infinigen Indoors.* CVPR. `10.48550/arxiv.2406.11824` — `StableAgainst`.
- Smelik et al. (2014). *A Survey on Procedural Modelling for Virtual Worlds.* CGF. `10.1111/cgf.12276`
  — split-grammar lineage, building interiors.
- Kutzias & von Mammen (2023). *Recent Advances in Procedural Generation of Buildings.* IEEE ToG.
  `10.1109/tg.2023.3262507`.
- Heese (2024). *Quantum Wave Function Collapse for PCG.* IEEE CG&A. `10.1109/mcg.2024.3447775` — HWFC.
- Sandhu, Chen & McCoy (2019). *Enhancing Wave Function Collapse with Design-level Constraints.* FDG.
  `10.1145/3337722.3337752`.

**Cited but not indexed:** Lagae & Dutré (2006). *An Alternative for Wang Tiles: Colored Edges versus
Colored Corners.* ACM TOG 25(4), 1442–1459.

---

## 8. Addendum (same day) — the corner problem is already in the corpus, and it layers

Added after a second pass prompted by the edge-vs-corner schema question.

### 8.1 Merrell assigns states to edges *and* vertices, in one propagation

`continuous` — Merrell & Manocha, *Continuous Model Synthesis* (SIGGRAPH Asia 2008) — is in the corpus
and states the edge/corner relationship directly:

> "We remove edge and vertex assignments that disagree with their neighboring assignments. **An edge
> assignment `(e, s_e)` agrees with an adjacent vertex state `s_v` only when `(e, s_e) ∈ s_v`, since
> vertex states are defined as sets of adjacent edge assignments.**"

and `10.1109_tvcg.2010.112`:

> "We keep track of a list of every possible state that could be assigned to **each edge and each
> vertex**."

So in model synthesis the corner constraint does not replace the edge vocabulary — the corner state is
*defined over* edge assignments, and agreement is set membership. Both live in one propagation loop.

**Consequence for staging:** the argument that moving tokens cell → face and then face → corner is "two
migrations" does not hold on this model. Cell → face is a migration of *where the token is read*. Corner
is an added *constraint over 4-tuples of the same tokens*. Additive, not a second rewrite of the same
data. That materially weakens the case for holding face tokens back.

### 8.2 Merrell also names the corner problem, under another name

The **incidence constraint** exists for exactly this reason:

> "Prior model synthesis techniques are limited to shapes which have only **trihedral vertices**...
> there are many simple shapes such as a pyramid or an octahedron that previous model synthesis
> techniques cannot generate."

> "States of trihedral vertices do not have this problem. They only use three half-spaces; three
> half-spaces require three planes to intersect; and **those three planes must intersect somewhere. But
> four planes may not intersect anywhere.**"

Three constraints always meet; four may not. That is structurally the same failure as four tiles each
satisfying every edge constraint and still disagreeing where they meet.

### 8.3 The caveat that still requires the paper

Merrell **layers** (vertex state = set of edge assignments). Lagae & Dutré's abstract describes corner
tiles as "square tiles with **colored corners**", which reads as *replacing* edge colors rather than
layering over them. If the corner formulation supersedes rather than subsumes the edge vocabulary, the
migration cost is different. **That — not "edge or corner" — is the question to read the paper for.**

Also note Merrell's setting is continuous half-space planes, not discrete tile tokens; the mapping to a
tile-token schema is an analogy, not a proof.

### 8.4 Locating the paper

`paper_download` fails because OpenAlex/Crossref report `download_urls: []` — there is no indexed OA
route. The authors host a copy on the KU Leuven graphics group page:

```
https://graphics.cs.kuleuven.be/publications/LD06AWTCECC/LD06AWTCECC_paper.pdf
```

Ingest path (the file must land in the papers directory before `scribe_convert` will see it):

```
curl -L -o ~/home-still/papers/la/LD06AWTCECC.pdf \
  https://graphics.cs.kuleuven.be/publications/LD06AWTCECC/LD06AWTCECC_paper.pdf
# then: scribe_convert(stem="LD06AWTCECC") -> distill_index(stem="LD06AWTCECC")
```

Abstract, via `paper_get` on `10.1145/1183287.1183296` (OpenAlex `W2051089395`, 125 citations), worth
having in the meantime:

> "Through their colored edges, Wang tiles enforce continuity with their direct neighbors. However,
> **Wang tiles do not directly constrain their diagonal neighbors. This leads to continuity problems
> near tile corners, a problem commonly known as the corner problem.** Corner tiles, on the other hand,
> do impose restrictions on their diagonal neighbors, and thus are not subject to the corner problem...
> **corner tiles are easier to tile**, textures synthesized with corner tiles contain more samples from
> the original texture, corner tiles **reduce the required texture memory by a factor of two**...
> Corner tiles result in cleaner, simpler, and more efficient applications."

"Easier to tile" cuts against the intuition that corner constraints cost more than edge constraints.

### 8.5 The two smaller schema questions

**Does a wall offer one face component or two? — Two.**

CGA's component split enumerates the faces of a scope; nothing collapses opposing faces
(`Comp("faces")`, `Comp("sidefaces")`). And CGA rule 15 —
`wall : Shape.visible("Street") ~ I("frontwall.obj")` — *selects* a face by query, which presupposes
more than one was available. Tutenel is the same shape one level down: features are plural and typed
per side, and a bookcase carries one `storage` feature **per shelf**. A corridor taking a sconce per
side is the same case as a bookcase taking a book per shelf.

The cost is real — a composite containing a wall must decide which of its faces is exterior — and CGA
answers that with the same mechanism as §3: a face occluded by the composite's own interior is not
exposed, via `Shape.occ("noparent")` scoping. Two faces plus occlusion scoping is one coherent design;
one face is a shortcut that has to be undone for the corridor case.

**Is `Offers::sockets` a component? — Apply Tutenel's test rather than deciding by symmetry.**

Tutenel separates **features** (geometric regions carrying layout semantics — *off-limits* overlaps
nothing, *clearance* overlaps only other clearance) from **services** ("the capacity of an object to
perform a particular action", queried as *"some object that provides heating"*). The
`2026-07-24` doc already identified collapsing these two axes as the root cause of the TV-on-bed bug:
*"An object's purpose and its mountable surfaces are different axes."*

So the criterion is: **does a socket have extent, such that two sockets can conflict spatially?**

- Yes → it is a feature → it is a component, and it belongs in the same overlap test as faces.
- No — it declares a capability ("this piece can host a light") → it is a service → not a component,
  and folding it in repeats precisely the collapse that produced the bug.

That gives the existing "and that is deliberate" note a criterion to be re-read against, rather than
being overturned by symmetry with the face decision.
