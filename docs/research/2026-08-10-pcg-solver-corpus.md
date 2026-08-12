# What the corpus holds on constraint-solver PCG

**Written 2026-08-10** for `docs/2026-08-10-constraint-solver-plan.md` §5.3. Everything below was read out of the local `home-still` corpus with `distill_search` → `catalog_read` → `markdown_read`. Every quotation is verbatim and is attributed to the **stem the chunk actually came from**, not to the title the catalog carries — several of these papers are catalogued with `title: null` and would never be found by a title search.

Two things in the plan need correcting before anything else, and both are in §1 and §4 below: **the plan's DOI for Karth & Smith points at a different paper**, and **the plan's "60% conflict rate" belongs to a different WFC modification than the one it is cited for**.

---

## Stem index

Read these directly rather than searching by title; four of the nine carry no catalogue title at all.

| Stem | What it actually is | Catalogue title |
|---|---|---|
| `10.1609_aiide.v18i1.21944` | Cooper 2022, **Sturgeon** | present |
| `10.1145_3102071.3110566` | Karth & Smith **2017**, *WaveFunctionCollapse is Constraint Solving in the Wild* | **null** |
| `10.1145_3337722.3341845` | Karth & Smith **2019**, *Addressing the Fundamental Tension of PCGML with Discriminative Learning* | **null** |
| `10.1145_3337722.3337752` | Sandhu, Chen & McCoy 2019, *Enhancing Wave Function Collapse with Design-level Constraints* | **null** |
| `10.1109_tciaig.2011.2158545` | Smith & Mateas 2011, *ASP for PCG: A Design Space Approach* | present |
| `pcgbook-ch08-asp-applications-mazes-levels` | Nelson & Smith, *ASP with applications to mazes and levels* (dup: `pcgbook_chapter08`) | **null** |
| `gameaipro2-ch26-rolling-own-finite-domain-constraint-solver` | Foged & Horswill, *Rolling Your Own Finite-Domain Constraint Solver* (dup: `GameAIPro2_Chapter26_...`) | **null** |
| `10.1609_aiide.v8i1.12511` | Horswill & Foged 2012, *Fast Procedural Level Population with Playability Constraints* | present |
| `10.1145_3235765.3235817` | Smith, Padget & Vidler 2018, graph-based action-adventure dungeons in ASP | **null** |

---

## 1. Sturgeon (`10.1609/aiide.v18i1.21944`) — held, complete, 1 converted page

Stem `10.1609_aiide.v18i1.21944`, Cooper, AIIDE 2022, converted from `papers/10/10.1609_aiide.v18i1.21944.pdf`, 25 chunks indexed. Content matches the title; the whole paper including appendix headings survived conversion. **Figures 6 and 7 did not** — the per-solver box plots that carry the head-to-head timings are images, so the numbers in §1.4 below are only those Cooper wrote in prose.

### 1.1 The mid-level API, verbatim (Table 1)

Cooper's own framing of it: *"A mid-level constraint API, consisting of only a few functions (described below and in Table 1), is used to express constraints over Boolean variables representing things such as tile placement and pathfinding properties of the level, which are then solved by a low-level solver."*

| Function | Description | SAT-style implementation | Answer Set implementation | SMT implementation |
|---|---|---|---|---|
| **MAKEVAR()** | Create a new Boolean variable. | | | |
| **MAKECONJ(ls)** | Create a representation of the conjunction (*and*) of the given literals. | Create new conjunction variable. | Create new conjunction variable. | And |
| **CNSTRCOUNT (vs, lo, hi, wt)** | Add a constraint that between *lo* and *hi* of the given variables (or conjunctions) *vs* are true, with weight *wt*. | *atMostK* native constraints if available, otherwise Boolean-encoded *atMostK* constraints† | Frontend: constrained choice rule / Backend: *add_weight_rule* | Pseudo-Boolean constraints PbLe, PbGe. |
| **CNSTRIMPLIESDISJ (l, ms, wt)** | Add a constraint that the literal (or conjunction) *l* implies the disjunction (*or*) of the literals (or conjunctions) in *ms*, with weight *wt*. | Clause | Frontend: rule / Backend: *add_rule* | Implies, Or |
| **SOLVE()** | Run the solver. | Soft constraints directly supported; multiple hard constraints (e.g. encoded cardinality) can be converted to a single soft constraints by adding a label variable‡ | Soft constraints supported via additional label variables‡ added to rules and given to *#minimize* (frontend) or *add_minimize* (backend). | Soft constraints directly supported. |
| **GETVAR(v)** | Get the value (i.e. true or false) of a variable. | | | |
| **GETOBJECTIVE()** | Get the value of unsatisfied soft constraint weights. | | | |

The two footnotes matter for the plan's L1, verbatim:

> † In this work we use PySat's kmtotalizer encoding (Morgado, Ignatiev, and Marques-Silva 2014); since PySat only supported hard native *atMostK* constraints, all such soft constraints must be encoded.
>
> ‡ In this work, label variables are used as additional variables added to hard constraints which can then themselves be used in soft constraints or optimizations, e.g. (Belov, Järvisalo, and Marques-Silva 2013).

And the shortcut note, which is the cheapest optimisation available to the plan's L1:

> In many special cases, shortcuts can be used: e.g. if there is only one literal in MAKECONJ, it can be used directly; if *lo* is 1 in CNSTR-COUNT, a disjunction can be used; SMT can use PbEq when *lo == hi*; etc.

**Mapping onto the plan's proposed signatures.** The plan's five functions are a faithful reduction: `var` ↔ MAKEVAR, `conj` ↔ MAKECONJ, `count` ↔ CNSTRCOUNT, `implies_any` ↔ CNSTRIMPLIESDISJ, `solve` ↔ SOLVE, `Solution::get` ↔ GETVAR, `Solution::unmet` ↔ GETOBJECTIVE. Nothing in Sturgeon's API is missing from the plan and nothing in the plan is invented. The plan's `weight: Option<u32>` is Cooper's `wt` with HARD as the sentinel.

### 1.2 The four design-rule families, verbatim

> We used four types of design rules for level generation. The design rules are collections of constraints that can be expressed using the mid-level API, which then uses a low-level solver to generate the level. For this work we used: tile rules, requiring one tile placed at each location; pattern rules, that provide relationships between nearby tiles learned from example levels; distribution rules, preferring the output tile distribution to be near the input distribution; and reachability rules, requiring a path through the level. Figure 1 shows the incremental effect of each type of rule.

**Tile rules (Table 2).** *"The basic tile rules create a Boolean variable for each allowable tile at each location, and then require that exactly one allowable tile is placed in each location. This is similar to a "one-hot" style encoding."* The API use is two lines: `MAKEVAR() → tile` per allowed tile per location, then `CNSTRCOUNT(tiles, 1, 1, HARD)` — *"Exactly one tile variable is true per location."* Table 2's footnote is a trick the plan should copy for its border cells: *"For convenience, all void tiles share a single variable that is constrained to be true (using CNSTRCOUNT)."*

**Pattern rules (Table 3).** Three clauses, and the third is the one that does the work:

```
MAKECONJ(tilesInPattern) → pattern
    A pattern is the conjunction of its individual tiles.

CNSTRIMPLIESDISJ(inputPattern, outputPatterns, HARD)
    If a placeable input pattern has output, and any output patterns are placeable:
    that input pattern implies at least one of its placeable output patterns.
CNSTRCOUNT(inputPattern, 0, 0, HARD)
    If a placeable input pattern has output, and no output patterns are placeable:
    that input pattern is false.

CNSTRCOUNT(allInputPatterns, 1, ∞, HARD)
    At least one placeable input tile pattern is true.
```

Cooper's own note on emulating WFC exactly: the `no-out3` template is *"A 3 × 3 input and no output. Since an input pattern must be present at each location where patterns are applied, this is meant to emulate WaveFunctionCollapse (Gumin 2016)."* That is the checkpoint the plan's §3 wants — Sturgeon has a named configuration whose job is to reproduce WFC's behaviour through the solver.

**Distribution rules (Table 4).** One line, and it is soft: `CNSTRCOUNT(countedTiles, min, max, SOFT)` — *"The number of tiles for a region and tag should be similar to the number desired, based on the example level."* Table 4's caption fixes the tolerance: *"In this work we use a low-weight soft constraint where min and max are ±50% of the desired number of such tiles."* The plan's `distribution_rules(..., target: &[f64], weight: u32)` matches, but the plan does not currently name a tolerance; **±50 % is Cooper's, and it is wide.**

**Reachability rules — see §1.3, because this is where the plan and the paper diverge.**

### 1.3 The reachability construction — and why it is *not* the plan's L3

Cooper's prose:

> The reachability rules are based on a graph constructed over the level grid. Roughly speaking, each location gets a node, and there is a directed edge from each node to nodes that the player could potentially reach from that node. However, edges can only be part of the reachability path if certain criteria of open (i.e. traversable by the player) and closed (i.e. not traversable) functional tiles are met at locations in the level. […] The reachability rules require a path in the graph from the start to the goal. This approach is similar to Aloul et al.'s pathfinding using SAT work (Aloul, Rawi, and Aboelaze 2006). However, their work is on an undirected graph, with all edges always usable, and uses optimization to find the shortest path.

Table 5 in full, verbatim (∀ is *"a loop or list over all the relevant variables"*, ⊕ is *"concatenation of variables into a list"*):

```
General:
  CNSTRCOUNT(validStartTiles,   1, 1, HARD)
  CNSTRCOUNT(validGoalTiles,    1, 1, HARD)
  CNSTRCOUNT(invalidStartTiles, 0, 0, HARD)
  CNSTRCOUNT(invalidGoalTiles,  0, 0, HARD)
      There must be exactly one start tile and one goal tile in valid locations,
      and none in any invalid locations.

For all edges:
  MAKEVAR() → edge
      Each edge has a variable for if it is reachable.

For all nodes:
  MAKEVAR() → node
  MAKEVAR() → open
  ∀openTile CNSTRIMPLIESDISJ(openTile, open, HARD)
  CNSTRIMPLIESDISJ(open, openTiles, HARD)
  ∀outEdge  CNSTRIMPLIESDISJ(¬node, ¬outEdge, HARD)
      Each node has a variable for if it is open; a node is open iff its
      corresponding tile is an open tile.
      A node being not reachable implies all its out edges are not reachable.

  ∀outEdge ∀needOpen   CNSTRIMPLIESDISJ(¬open_needOpen,  ¬outEdge, HARD)
  ∀outEdge ∀needClosed CNSTRIMPLIESDISJ( open_needClosed, ¬outEdge, HARD)
      For an edge, a node required to be open being closed, or a node required
      to be closed being open, implies the edge is not reachable.

  CNSTRCOUNT(inEdges,  0, 1, HARD)
  CNSTRCOUNT(outEdges, 0, 1, HARD)
      A node has at most one reachable in edge and one reachable out edge.

  MAKECONJ(¬startTile ⊕ ∀inEdge ¬inEdge) → noIn
  CNSTRIMPLIESDISJ(noIn, ¬node, HARD)
      A node that is not the start node and has no incoming reachable edges
      is not reachable.

  CNSTRIMPLIESDISJ(startTile, node, HARD)
  ∀inEdge CNSTRIMPLIESDISJ(startTile, ¬inEdge, HARD)
      The start tile locations's node is reachable and has no incoming
      reachable edges.

  CNSTRIMPLIESDISJ(goalTile, node, HARD)
  ∀outEdge CNSTRIMPLIESDISJ(goalTile, ¬outEdge, HARD)
      The goal tile locations's node is reachable and has no outgoing
      reachable edges.
```

**Read the two `CNSTRCOUNT(…, 0, 1, HARD)` lines carefully. This encodes a *path*, not a *reachable set*.** In-degree and out-degree are both capped at one, the start has no in-edge, the goal has no out-edge — the reachable subgraph is a simple chain. The plan's L3 wants `outside[c]` = *the whole set of cells reachable from the border*, which is a flood fill with unbounded degree. **Sturgeon's Table 5 is not that encoding and cannot be copied for it.** The plan's three-line sketch is closer to Nelson & Smith's `linked/2` (§3.2 below) than to anything in Sturgeon.

**And Sturgeon's foundedness is *not* sound — Cooper says so and draws the failure.** The plan's §L3 asserts that circular justification is a SAT-only problem and that *"ASP's minimal-model semantics gives this free; that is why Cooper used clingo."* That is not what the paper reports. The `noIn → ¬node` rule is exactly the support rule the plan sketches, written in the same contrapositive shape for all three back-ends — and it leaks:

> We noted some potentially interesting side-effects of this approach. First, it is only required that a path can be found; it does not have to be short or direct. Thus, the solver can find quite circuitous paths. **Second, in addition to the path from start to goal, the solver can include additional closed cycles off the main path in the solution.** For clarity, these cycles are not shown in most figures, but are shown in Figure 1(d).

Figure 1's caption is the paper drawing its own unfounded cycles: *"Reachability path shown in red (unused closed cycles shown in gold)."*

The consequence for the plan is direct and is the most load-bearing finding in this document. Cooper tolerates unfounded cycles because a floating loop off the main path is *harmless* when the only question is "does a path exist". For enclosure, an unfounded cycle is a set of cells falsely claiming `outside`, which is exactly the wrong answer. **So the plan cannot inherit foundedness from Sturgeon on any back-end, clingo included** — the mid-level API is solver-agnostic by construction, which means it cannot use clingo's minimal-model semantics even when running on clingo. The level/rank encoding the plan flags as "the single most likely place to get it subtly wrong" is required, not optional, and it is required regardless of solver choice. The only encoding in the corpus that gets foundedness from ASP semantics is Nelson & Smith's, and it gets it by being written as a genuine recursive rule with a positive head (§3.2).

### 1.4 Solve times and scaling — what is actually reported

Method, verbatim: *"In the following evaluations, for each game setup (game, flow, pattern and reachability template, region definition, level size, and solver), we generated 25 levels. […] **We aimed for level sizes that could be generated in about 10s or less by most solvers. Evaluations were run on a 2018 MacBook Pro.**"* The system is Python: *"The system is implemented in Python; the mid-level solver API is implemented as Python functions and uses the Python modules of the low-level solvers."*

Sizes evaluated (Figure 6's table survived; its timings did not):

| Band | Game | Size | Cells | Reachability template |
|---|---|---|---|---|
| small functional | icarus | 8×16 | **128** | platform |
| small functional | mariobros | 14×16 | 224 | platform |
| small functional | cave | 20×20 | 400 | maze |
| larger | icarus | 20×16 | 320 | platform |
| larger | marioland | 16×30 | 480 | platform |
| larger | supercat | 30×20 | 600 | supercat |
| larger | mariobros | 14×60 | 840 | platform |
| larger | cave | 30×30 | 900 | maze |
| larger | zelda | 33×32 | 1056 | maze |

**For the plan's 12×12: 144 cells sits between Sturgeon's two smallest evaluated functional grids (icarus 8×16 = 128 and mariobros 14×16 = 224), both of which are inside a target band of "about 10s or less by most solvers" — in Python.** A Rust implementation of a strictly smaller problem has margin. The corpus gives no reason to expect a 12×12 to be slow.

Solver-comparison findings, verbatim:

> Results are in Figure 6(top). clingo-fe and z3 were much slower than the other solvers, and pysat-fm appeared generally slower than pysat-rc2; thus we excluded them from further evaluations.

> This example highlights a benefit of portfolio solving: clingo-be appears faster for cave while pysat-rc2 appears faster for icarus; however, using both, the portfolio solver performs well across all these games (though there was some overhead in managing the multiple solvers).

Note what that says about the plan's §5.1 solver question: **the ASP *frontend* was the slowest thing measured** — clingo-fe was dropped alongside z3. Cooper's fix was to bypass grounding entirely: *"For clingo-fe, we used the standard text-based "front-end" Answer Set Programming language with grounding and solving. In practice we found this approach to run quite slowly. Thus, for clingo-be, we used clingo's "backend" API directly to construct the program for the solver, and bypass the grounding step."*

The absolute numbers Cooper wrote in prose, all on the `mario_bros (ring)` setup unless stated:

| Application | Size | Median | Maximum |
|---|---|---|---|
| tall-pipe | 14×24 | 3.1 s | 3.8 s |
| 3? (exactly 3 question blocks) | 14×24 | 2.0 s | 2.1 s |
| max-gap | 14×24 | 2.5 s | 3.7 s |
| void (out tag grid, soft patterns) | 16×40 | 3.1 s | **25.2 s** |
| infill | 14×32 | 2.6 s | 3.0 s |
| link | 14×37 | 3.0 s | 3.1 s |
| **repair** | 14×18 | **10.5 m** | **24.8 m** |
| flowers-1 (WFC emulation, image only) | — | 30.3 s | 36.2 s |
| flowers-7 | — | 3.6 s | 4.3 s |
| skyline-15 | — | 6.9 s | 14.9 s |
| skyline-20 | — | 6.0 s | 7.0 s |

Image generation for the level examples *"took <1s for all these."* Repair is the outlier and Cooper flags it: *"These examples took notably longer; median and maximum functional generation times were 10.5m and 24.8m."* The conclusion names it as future work: *"explore further performance improvements, particularly to level repair."*

**The two danger signals for the plan are `void` and `repair`, and both are shapes the plan will hit.** `void` is a 3.1 s median against a 25.2 s maximum — an eight-fold spread on the same setup, from soft pattern constraints. `repair` is the same machinery told to make minimal changes to an existing grid under soft constraints, and it costs minutes. The plan's L4 takes a `Map`, a region and `Wishes`, and its `unmet()` story — *"everything except the north corridor"* — is a soft-constraint optimisation over an existing artefact, which is structurally the repair case, not the generate case. Budget accordingly and measure the *maximum*, not the median.

Finally, the expressive-range experiment, which is the closest thing in the corpus to the plan's §4 measurement loop:

> Using the mario_bros (ring) setup, we explored the coverage along two dimensions, using number of gap tiles and number of solid tiles, with 6 ranges per dimension. A time limit of 10m was used for each 14 × 24 level. Of the 36 possible levels, 19 were found. Most of the levels not found required few or many solid tiles; 7 timed out. The total functional level generation took 86.2m, or 2.4m per possible level.

**19 of 36 found and 7 timing out at a ten-minute budget** is the honest baseline for what "constrain a property to a range and generate" costs. The plan's gate is *"> 20 % non-convergence → nothing else is interpretable"*; Cooper's own expressive-range sweep had 19 % hard timeouts and 47 % cells unfilled. Worth knowing before the gate is read as a verdict on the encoding.

---

## 2. Aloul, Rawi & Aboelaze — **not in the corpus**

Searched `distill_search` for the title, the authors, and the concept ("pathfinding using Boolean satisfiability", "reachability encoding grid"). **No hit.** The only occurrences of the name anywhere in the corpus are inside Sturgeon's own reference list, which is why a naive search appears to find it:

> Aloul, F. A.; Rawi, B. A.; and Aboelaze, M. 2006. Identifying the shortest path in large networks using Boolean satisfiability. In 2006 3rd International Conference on Electrical and Electronics Engineering, 1–4.

Metadata confirmed via `paper_search` (no download performed):

- **Title:** Identifying the Shortest Path in Large Networks using Boolean Satisfiability
- **Authors:** Fadi A. Aloul, Bashar Al Rawi, Mokhtar Aboelaze
- **Venue:** 2006 3rd International Conference on Electrical and Electronics Engineering (ICEEE), Mexico, September 2006
- **DOI:** `10.1109/iceee.2006.251924`
- **OpenAlex:** `W2035228826`, 9 citations
- **OpenAlex/CORE/CrossRef `download_urls`: empty** — no OA link from the metadata providers.

**An open-access PDF does exist**, on the first author's own publications page. Recorded, not downloaded:

- **`http://www.aloul.net/Papers/faloul_iceee06.pdf`** (author-hosted, direct PDF)
- Paywalled of record at `https://ieeexplore.ieee.org/document/4018009/`
- Also mirrored at `https://www.academia.edu/13868397/`

From the abstract, the part that bears on the plan: *"it is difficult to incorporate user-specific conditions on the solution when using Dijkstra's algorithm. Such conditions can include forcing the path to go through a specific node, forcing the path to avoid a specific node, using any combination of inclusion/exclusion of nodes in the path […] In this paper, we show how to formulate the shortest path problem as a SAT problem."*

**How much this matters: less than the plan implies.** Cooper says his approach is only *"similar to"* Aloul's, and names three differences in one sentence — *"their work is on an undirected graph, with all edges always usable, and uses optimization to find the shortest path."* All three differences run away from what the plan needs. The plan wants edges that are *conditionally* usable (a seam is open only if no wall blocks it) and it wants no optimisation over path length at all. Fetching this paper is a low-value errand; it is a shortest-path encoding, and the plan's enclosure problem is a reachable-set problem.

---

## 3. ASP for PCG — well covered, and one chapter is worth more to the plan than Sturgeon

### 3.1 Smith & Mateas is held (`10.1109_tciaig.2011.2158545`)

The canonical reference, present and indexed, title and content agreeing. Abstract, verbatim:

> Procedural content generators for games produce artifacts from a latent design space. This space is often only implicitly defined, an emergent result of the procedures used in the generator. In this paper, we outline an approach to content generation that centers on explicit description of the design space, using domain-independent procedures to produce artifacts from the described space. By concisely capturing a design space as an answer set program, we can rapidly define and expressively sculpt new generators for a variety of game content domains.

The passage the plan should read before committing to a solver, because it is the only characterisation of runtime behaviour in the corpus written from the ASP side:

> Attempts to characterize the runtime performance that should be expected from common answer set solvers have yielded results very similar to those seen for SAT solvers [33]. That is, while solvers generally employ algorithms with worst case exponential complexity (in terms of a program's grounded size), solvers will terminate very quickly on a wide range of problem instances. Only when random programs have a mix of atoms and rules that approach a critical ratio (reminiscent of the "phase transition" for SAT instances) does a solver actually encounter exponential blowup in its search process. On ASP terms, the "hardest" problems appear to be those where exactly one answer set from an extremely large space of possibilities is the only solution. Anecdotally, we have found realistic PCG problems (large chromatic maze generation not included) such as those surveyed in the next section to fall on the "easy" size of the phase transition. That is, their constraints are relatively easy to satisfy (admitting an estimated number of valid answer sets in the quadrillions for the case of Variations Forever), but the way in which they are satisfied leads to interesting game content.

**"The 'hardest' problems appear to be those where exactly one answer set from an extremely large space of possibilities is the only solution"** is a description of what the plan risks building. A 12×12 with a hard enclosure constraint, a learned support relation and a distribution target is a search for a narrow satisfying set inside a large space. If the plan tightens `Wishes` until only a handful of grids qualify, this is the regime it lands in — and the corpus says that is where blowup lives.

The paper also contains a complete runnable ASP maze generator in its appendix: *"Below is the complete source for our ASP-based chromatic maze generator (tested in Clingo 2.0.5). To recreate our generation of the globally optimal 6-by-6 maze, invoke `clingo appendix.ans -c min_solution=35`."*

### 3.2 Nelson & Smith, *ASP with applications to mazes and levels* — **the encoding the plan's L3 actually wants**

Stem `pcgbook-ch08-asp-applications-mazes-levels` (byte-identical duplicate at `pcgbook_chapter08`). PCG Book chapter 8. Neither copy carries a catalogue title, which is presumably why the plan does not cite it. It is the single most directly useful document in the corpus for L3.

The whole reachability construction, verbatim, as `maze-reach.lp`:

```
linked(1,1).
linked(X,Y) :- parent(X,Y,DX,DY), linked(X+DX,Y+DY).
:- dim(X;Y), not linked(X,Y).
```

with the prose that explains it:

> To make sure we only see valid trees, we should enforce the property that the root is reachable from every tile on the grid. Figure 8.3 uses a fact, a recursive rule, and an integrity constraint to accomplish this. The linked(X,Y) property holds trivially for the root of the tree. Any tile that has a parent that is linked is linked as well. Finally, if there is some tile which does not have the linked property, something is wrong with the current assignment of parent directions and this possible world should be rejected.

And the failure it fixes, which is the plan's circular-justification hazard shown as a picture — Figure 8.2's caption: *"When each tile in the maze is assigned a random parent, typical outputs show several disconnected components. Some tiles on the edges of the maze even point to a parent cell outside of the maze."* Figure 8.4's: *"After adding the reachability constraint for each tile, the desired tree network appears. This program captures exactly the set of all perfect mazes of a given width."*

Three things the plan should take from this.

**First, this is the shape `outside[c]` should have.** Three lines: a base fact at the border, one recursive rule that propagates across an open seam, one integrity constraint. The plan's sketch is line-for-line this program with `border(c)` for `linked(1,1)` and `seam_open(c,n)` guarding the recursion. That is a good sign — but it is a good sign about *this* citation, not about Sturgeon.

**Second, the foundedness really is free here, and it is free for a reason the plan should state precisely.** `linked/2` appears in the *head* of a rule with a *positive* body. Minimal-model semantics will not conclude `linked(a)` from `linked(b)` and `linked(b)` from `linked(a)` with no base case, because that model is not minimal. Cooper's `noIn → ¬node` has a *negative* head — it is an integrity constraint, not a founding rule — and integrity constraints do not found anything on any back-end. **The distinction is head polarity, not solver family.** A plan that ports Sturgeon's API faithfully will get Sturgeon's cycles, on clingo, unless it writes the recursion as a rule rather than as a constraint — which the solver-agnostic mid-level API is precisely designed not to let it do.

**Third, the chapter has the soft-constraint pattern too**, as `maze-bias.lp`:

```
% soft style preferences : minimize vertical links
vertical(X,Y) :- parent(X,Y,0, 1).
vertical(X,Y) :- parent(X,Y,0,-1).
#minimize { vertical(X,Y) }.
```

with a note that is directly relevant to the plan's determinism constraint in §6:

> Although such statements are typically read as implying an optimality constraint (that only globally optimal solutions should be emitted), most answer set solvers will emit a series of answer sets they find along the way to finding one such optimal solution. By stopping the solver once it gets close enough or runs for enough time, we can implement approximate optimisation within this framework as well.

**Stopping early is a nondeterminism source.** "Runs for enough time" is wall-clock, which cannot appear anywhere in a build that has to hash-match. If the plan ever wants an anytime budget, it must be a *deterministic* budget — conflict count, decision count, propagation count — never elapsed time.

The chapter's summary line, which is the citation the plan's §L3 should carry: *"Constraints such as the reachability constraint can be implemented recursively."*

### 3.3 Other ASP/solver PCG in the corpus

- **`10.1145_3235765.3235817`** — Smith, Padget & Vidler, FDG '18, graph-based action-adventure dungeon levels in ASP. Contains a compact survey paragraph of the whole ASP-for-PCG line, and its own choice-rule bounds: *"Within the choice rule for each directly-generated node type are specified an upper bound and lower bound on expected node counts."* Also names a room-assembly precedent: *"Smith and Bryson [16] describe a system using ASP to assemble room modules from a pool of pre-generated templates into a consistent dungeon layout according to connectivity."*
- **`10.5121_ijaia.2023.14302`** — Xu & Morris, metroidvania levels with clingo + Perlin noise. Its value is operational rather than technical: *"answer set solvers such as clingo need to go through complicated installation steps on any local machine in which it is used to function properly. Therefore, it becomes difficult for other implementations to work for the wider public."* Their workaround was to host clingo on a server. That is a datapoint against the clingo option in the plan's §5.1 and against `emerge-core`'s `ALLOWED_DEPS` ratchet in §6. They also claim the property the plan is chasing: *"our method has strength in its ability to both generate and verify the map structure / geometry in the same step, which guarantees that the level that is generated satisfies any valid constraint we give it."*
- **`10.1609_aiide.v19i1.27525`** — Madkour, Holtzen, Harteveld, Marsella & Martens, *Probabilistic Logic Programming Semantics For Procedural Content Generation*, AIIDE 2023. Held; not read in depth here. If the plan's §6 problem — *"variety must come from varying the problem per seed"* — turns out to be awkward, this is the paper in the corpus about putting the probability *inside* the declarative semantics instead of outside it.

---

## 4. Sandhu, Chen & McCoy (`10.1145/3337722.3337752`) — held, and the plan has conflated two of its results

Stem `10.1145_3337722.3337752`, catalogue title `null`, one converted page, 14 chunks. Content is unmistakably *Enhancing Wave Function Collapse with Design-level Constraints*, Sandhu, Chen & McCoy (UC Davis), FDG '19. **Table 2 did not survive conversion** — see below, it matters.

### 4.1 "constraints that can work over any distance" — the quote is accurate

Verbatim from the abstract: *"First, we extend the local constraint reasoning by incorporating constraints that can work over any distance and non-spatial constraints. Next, we further manipulate the generative space by introducing weight recalculation and dependencies."*

Four modifications were built and measured separately:

1. **Weighted choice** — internal, changes how a tile is picked once a cell is chosen.
2. **Non-local constraints** — items with `name, weight, frequency, dependency`, plus upper/lower distance bounds between paired items, implemented by a forced extra observation step.
3. **Weight recalculation** — re-weight tiles inside a triggered area, recompute entropy across the wave.
4. **Area propagation** — ban a category of objects inside a computed sub-area.

### 4.2 The 60 % number: verified, but it belongs to **area propagation only**

Verbatim, and this is the whole of the relevant passage:

> Area propagation is a modification that increased the contradiction rate. Instead of testing runtime and memory, this paper evaluated its conflict rate against the area of interest. […] The tested areas are 5x5, 7x7, and 9x9 tiles. These sizes were chosen because the smallest area around a tile that does not include its immediate neighbors is 5x5, area size increases by two for both height and width simultaneously, and the test map size is 10x10. **Other map sizes were not used because the rate of producing a map drops close to 0 maps/generation when map size is above 10x10.** 50 runs were conducted for each propagation test area for 15 times. The resulting mean and standard deviation of successful map generations given in percentages for each test area is summarized in Table 2.

> As can be seen, the area of propagation has no effect on the conflict rate. Regardless, **the resulting conflict rate is around 60% for a map size of 100 tiles** with the parameters and inputs defined previously. Further testing will need to be conducted to give a complete analysis of area propagation. However, it can be concluded that **area propagation is best used as a constraint for design time rather than runtime due to the high conflict rate.**

So: **the number is real, 100 tiles is indeed a 10×10 map, and the plan quotes it correctly. But it attaches to area propagation — one of four modifications — and not to "constraints that can work over any distance," which is a different modification with different, much better numbers.**

Two caveats on the number itself, both worth carrying:

- **Internal inconsistency.** The method says Table 2 reports *"successful map generations given in percentages"*; the prose then calls the same quantity a *"conflict rate"* of ~60 %. These are complements. Either Table 2 reads ~40 % success (and the prose is right) or it reads ~60 % success (and the prose mislabels). **Table 2 is not in the converted markdown**, so the corpus cannot settle it. Cite the 60 % with that hedge or not at all.
- **The stronger sentence is not the 60 %.** It is *"Other map sizes were not used because the rate of producing a map drops close to 0 maps/generation when map size is above 10x10."* Area propagation does not degrade above 10×10; it stops working. **The plan's 12×12 is above 10×10.** If anyone proposes area-propagation-style WFC extension as the cheap alternative to a real solver, that sentence is the answer.

### 4.3 What non-local constraints actually cost — the numbers the plan should have used

Measured separately, 50 runs per size, map sizes 10×10 / 20×20 / 30×30 / 40×40 / 50×50 / 100×100, NodeJS:

> As depicted in Figure 9a, non-local constraints increased execution time that ranged from about 1ms for 100 tiles to 440ms for 2500 tiles. In fact, the trend for the increase in runtime relative to the map size can be fitted to a polynomial regression (Figure 9a), which provides a way to extrapolate time cost to higher map sizes.

> As seen from Figure 9a, time cost for creating a map size of 2500 can be as much as 0.55 seconds while it is 1 ms and 15 ms for a map size of 100 and 400 tiles, respectively. This means, depending on the tolerability of lag, a technical artist may want to evaluate the benefit of adding non-local constraints to WFC at map sizes that exceed 400 tiles.

> The average memory usage increased from 1MB to about 20MB for map sizes of 100 tiles to 2500 tiles. The largest memory usage average is exhibited by a map size of 10k tiles (Figure 9b), which is about 90MB.

Weight recalculation is worse, and the authors explain why in terms the plan will recognise: *"The increase is attributed to the fact that the entropy of the entire wave is recalculated each time a weight is updated. As a result, the runtime of up to N-1 tiles is added to the total execution time. The worst-case scenario is when the entire wave entropy is recalculated after each observation/propagation loop, which adds a runtime of (N-1)! tiles."*

The conclusion, which is the fair summary of the paper: *"We have provided tests that show that **most** of these constraints are efficient enough to be used at runtime. **Some** have high conflict rates and would be better for integration within a mixed-initiative tool."*

### 4.4 Was WFC-with-extensions ever a viable alternative? — the honest reading

**On cost, yes; on capability, no — and the capability gap is the one that matters here.**

Non-local constraints at 144 cells would cost single-digit milliseconds and ~1 MB. That is not the objection. The objection is what those constraints *are*: item frequency counters, item-to-item dependencies, and upper/lower distance bounds on paired placements. Read §3.3.1 of the paper and the whole vocabulary is *"a key or lock can be placed and then a forced observation of the complement item occurs N range of tiles away."* **There is nothing in Sandhu et al. that expresses connectivity, enclosure, or any property of a region.** Not a weaker version of it — none of it. The mechanism is a forced extra observation inside a computed sub-area, which is a *local* operation relocated, not a global constraint.

The one modification that reaches for a region-scoped property — area propagation — is the one that collapses above 10×10 and that its own authors relegate to design time.

So the plan's §0 conclusion survives contact with this paper, but the supporting sentence needs rewriting. It is not that WFC-with-extensions is too slow. It is that **the extensions in the literature extend WFC's reach in *distance*, never in *arity*** — they still constrain one placement against another placement, and enclosure is a predicate over a set. See also `10.1145_3102071.3110566` §5.2 (next section), where Karth & Smith test a genuine global constraint on WFC's search discipline and it breaks.

---

## 5. Karth & Smith — the plan cites the wrong DOI

### 5.1 The correction

The plan's §0 says: *"WFC is a greedy, non-backtracking approximation of constraint solving (Karth & Smith)"*, and the research brief identifies that as `10.1145/3337722.3341845`. **That DOI is a different paper.**

- **`10.1145/3337722.3341845`** = Karth & Smith, **2019**, *Addressing the Fundamental Tension of PCGML with Discriminative Learning*, FDG '19. Held as `10.1145_3337722.3341845`, 9 pages, 25 chunks. Verified by reading its page 1, which carries its own ACM reference format line: *"Isaac Karth and Adam M. Smith. 2019. Addressing the Fundamental Tension of PCGML with Discriminative Learning. In The Fourteenth International Conference on the Foundations of Digital Games (FDG '19) […] https://doi.org/10.1145/3337722.3341845"*. It is about training WFC with positive and negative example fragments — *"we propose the use of discriminative models, which capture the validity of a design rather the distribution of the content, trained on positive and negative example design fragments."* Useful, but not the constraint-solver-framing paper.
- **`10.1145/3102071.3110566`** = Karth & Smith, **2017**, *WaveFunctionCollapse is Constraint Solving in the Wild*, FDG '17. Held as `10.1145_3102071.3110566`, one converted page, 17 chunks. Catalogue title `null`. **This is the paper the plan means.**

**Fix the DOI in the plan.** (`10.1145_3337722.3337752`, Sandhu et al., is a third FDG '19 paper in the same proceedings prefix — the three are easy to mix up and two of the three have no catalogue title.)

### 5.2 What the 2017 paper says about WFC being a constraint solver

The abstract, verbatim:

> Maxim Gumin's WaveFunctionCollapse (WFC) algorithm is an example-driven image generation algorithm emerging from the craft practice of procedural content generation. In WFC, new images are generated in the style of given examples by ensuring every local window of the output occurs somewhere in the input. **Operationally, WFC implements a non-backtracking, greedy search method.** This paper examines WFC as an instance of constraint solving methods.

The identification, in the paper's own terms:

> Taken together, we can see WaveFunctionCollapse as a constraint solving algorithm. Indeed, Gumin occasionally describes his algorithm this way. It uses the minimum remaining values (MRV) heuristic to select a variable to decide next. For decisions, it uses the heuristic of choosing patterns according to their distribution in the original image.

The CSP mapping, spelled out:

> Constraint satisfaction problems (CSPs) are typically defined in terms of decision variables and values. In the context of WFC-style image generation, there is a variable associated with each location in the output image. […] For the task addressed by WaveFunctionCollapse, the values are associated with the discrete set of unique local patterns in the input image. […] Constraints relate the legal combination of values that a set of variables might take on in a valid assignment.

And the propagation is AC-3 by another name: *"Like AC3, WFCs propagation procedure implements arc consistency […] As such, propagation proceeds via an algorithm recognizable from a graphics perspective as a flood fill."* On backtracking, unambiguously: *"Gumin's algorithm does not implement local backtracking and instead globally restarts in the rare case a conflict is reached."* And by contrast with Tanagra: *"By contrast, Gumin's WaveFunctionCollapse does not backtrack."*

**So the plan's premise is exactly, quotably right.** "WFC is a greedy, non-backtracking approximation of constraint solving" is the paper's own abstract sentence plus its own mechanism section.

### 5.3 The experiment that is the plan's argument, already run in 2017

This is the section that most justifies the plan, and the plan does not currently cite it. Karth & Smith reimplemented WFC's problem in ASP and asked how much of WFC's success comes from the greedy discipline.

Their formulation is two rules:

```
1 { assign(X,Y,P):pattern(P) } 1 :- cell(X,Y).

:- adj(X1,Y1,X2,Y2,DX,DY),
   assign(X1,Y1,P1),
   not 1 { assign(X2,Y2,P2):legal(DX,DY,P1,P2) }.
```

On the plain problem, greedy is *free*, and this is the surprising half of the result:

> Surprisingly, Clingo encounters zero conflicts during search for the selected scenarios. This result still holds if we tell Clingo to make random choices for each selected location (something needed to achieve varied outputs for gameplay purposes). **This suggests that the strength of WFC comes from constraint propagation (removing bad choices from variable domains before they are considered for assignment) rather than the entropy heuristic.**

Table 1 confirms it across three scenarios (10 × 48×48 images each): Platformer 486,238 vars / 1,976,634 constraints → **0 conflicts** under both VSIDS and heuristics-disabled; Skyline 698,544 / 3,009,600 → 0; Flowers 485,958 / 1,909,342 → 0. Footnote 23: *"all non-timeout solving times were under two seconds using single threaded search on a Early 2011 MacBookPro with a 2.2 GHz Intel Core i7 processor."*

Then they add **one global constraint** — every input pattern must be used at least once:

```
:- pattern(P), not 1 { assign(X,Y,P).cell(X,Y) }.
```

and it breaks:

> Experimentally, we found that while adding this constraint did not significantly impact the number of conflicts encountered for the Flowers and Platformer scenarios, **it leads to hundreds of conflicts for the Skyline scenarios. When Clingo is instructed to globally restart after each conflict (mimicking WFC), it cannot find a solution within the one-minute timeout window. However, if backtracking is allowed (the default behavior of Clingo), the constraint can be quickly resolved by adjusting local choices.**

And then they name, as future work, the exact constraint the plan is trying to build:

> In deeper game design applications of WaveFunctionCollapse that attribute gameplay semantics to what are just pixel colors in the image generation task, we expect the demand for global constraints like this to grow. For example, consider an application that attempts to use WFC to generate an explorable environment. **It seems desirable to be able to ask the search algorithm to enforce global reachability constraints: every location which the player might occupy should have a feasible path from the initial location in the environment.** A designer might specify this by identifying a certain pixel color in the input image and flagging that color as **needing to form a single connected graph (a global constraint). A balance of local backtracking and global restarts will be needed in the search algorithm to efficiently generate designs satisfying this constraint.**

Their footnote 24 on that passage points at the real-world workaround, which is what this project's dungeon path effectively does: *"Such as could potentially be useful for the rogue-like dungeons in Caves of Qud, which currently uses a multi-pass approach that adds doorways and connections after WFC runs to ensure connectivity."*

**This is the strongest single citation in the corpus for the plan's §0.** It is a controlled experiment, on WFC's own problem, isolating the one variable the plan cares about: with only local constraints, greedy non-backtracking search is *free* (zero conflicts); add one global constraint and greedy-with-global-restart *cannot find a solution at all* while backtracking resolves it quickly. That is the plan's expressive-range result — 128 solves, zero enclosed regions — predicted from first principles nine years earlier, with the same diagnosis and the same prescription.

The conclusion also anticipates the plan's "swap the solver, keep the kit" move:

> Through experiments with the ASP surrogate implementation, we show that WFC's choice of heuristic and decision to only apply global restarts of search are reasonable choices for the original discrete image generation task, **but they are not critical going forward.** […] We assert that search in the space of partial assignments and constraint propagation are the primary strengths of WFC.

---

## 6. Held, relevant, and not in the plan

Four documents the plan does not cite and probably should.

**`gameaipro2-ch26-rolling-own-finite-domain-constraint-solver`** — Foged & Horswill, *Rolling Your Own Finite-Domain Constraint Solver*, Game AI Pro 2. This is the chapter Karth & Smith explicitly hand to a game audience: *"For a game-focused audience, we refer the reader to the Game AI Pro 2 book chapter "Rolling Your Own Finite-Domain Constraint Solver" for more details."* It is a build-it-yourself walkthrough in six algorithm steps ending at *"Forward Checking with Backtracking and Undo"*, with an AC-3 variant (*"This propagation algorithm is a variant of Mackworth's arc consistency algorithm #3 (AC-3)"*), a trail-based undo stack (*"Implementing undo for variables is pretty much like implementing undo for a word processor"*), and — directly useful for the plan's `count` — a worked `AtMostConstraint` with a companion "at least":

```
class AtMostConstraint {
    Variable[] variables;
    FiniteDomain constrainedValue;
    int limit;
    public override bool Narrow() { … }
}
```
> An "at least" constraint is implemented similarly, but rather than monitoring how many variables can only have the specified value, it monitors how many haven't yet ruled it out.

Its motivating example is this project's problem almost verbatim: *"suppose you are building a rogue-like or a dungeon crawler and you want to decide what items and enemies to put in what rooms. […] You could probably write an ad hoc algorithm to do that and get it to work eventually […] And it could have some very subtle bugs like making unsolvable levels once every 700 runs."*

**`10.1609_aiide.v8i1.12511`** — Horswill & Foged, *Fast Procedural Level Population with Playability Constraints*, AIIDE 2012. Constraint propagation for level population with **path constraints**: *"We introduce a notion of path constraints, which bound some function over the possible paths a player might take, and show how to efficiently place objects while guaranteeing path constraints."* Their extension is interval methods over finite domains — *"finite-domain solvers have difficulty handling numeric data. We show that by extending finite-domain techniques with interval methods, we can significantly reduce both the execution time and the memory footprint of the system, allowing it to be used even on low-end platforms."* If the plan's `Wishes` ever grows a numeric field (a density, a budget), this is the paper on doing that inside a propagator rather than around it.

**`10.48550_arxiv.2308.07307`** — Nie, Zheng, Zhuang & Song, *Extend Wave Function Collapse to Large-Scale Content Generation*. Carries the complexity statement the plan's §0 implies but does not state: *"Suppose the number of the tileset is |T| = d, the time complexity of WFC is O(d^{M×N} + (M×N)²d³), which grows exponentially with the size of the generation grid."* Also documents the industry failure mode by name: *"Marian Kleineberg, the creator of infinite city, mentioned that WFC has the problem that some places, although shown to the player, require backtracking and re-generation due to conflicts."*

**`10.1109_access.2022.3168832`** — Códices, Andrade, Silva & Fachada, *Procedural generation of 3D maps with snappable meshes*. Connector-compatibility snapping via pin count and colour: *"Compatibility between connectors from different pieces is defined by two parameters set by the designer: pin count and color."* This is the nearest thing in the corpus to this project's `Interface`/seam vocabulary, and it explicitly positions itself against WFC on comprehensibility: *"than WFC, which several users found difficult to grasp and refactor."* It is a constructive placer, not a solver, so it is background rather than a source.

---

## 7. What the corpus does **not** cover

Stated plainly, because the plan should know which of its claims rest on internal reasoning. Each of these was searched for directly and the search returned nothing on-topic.

**Enclosure as a generative constraint. Nothing.** The corpus has reachability-to-a-goal (Sturgeon, Nelson & Smith, Horswill & Foged), connectivity-of-a-tree (Nelson & Smith), and connectivity-as-future-work (Karth & Smith 2017 §5.2). It has **no** paper that constrains a generated grid to contain interior regions unreachable from outside. The nearest hits were an evolutionary dungeon generator whose fitness rewards connectivity (`pcgbook-ch09-representations-search-based-methods`, where connectedness is a *probability* managed by sparse initialisation: *"This gene is initialised to 20% ones, 80% zeros to make the probability the map is connected high"*) and a mesh-processing "closed" test. **The plan's L3 is an inversion nobody in the corpus has written down.** That is not a reason not to build it, but it means the `want: f32` semantics, the choice of hard-vs-soft, and the interaction between enclosure and the distribution target are all unsourced design decisions.

**Founded reachability in SAT. Nothing.** No paper on unfounded sets, loop formulas, level/rank encodings, or the ASP-to-SAT translation of recursion. Searched for all of these by name. The plan's *"~8 bits at 144 cells"* level-encoding sketch has no citation available and must be justified on its own terms — and per §1.3 above, this gap is worse than the plan thinks, because Sturgeon does not solve foundedness either.

**Cardinality encodings. Nothing.** No totalizer, no sequential counter, no commander/product encoding, no MiniCard, no pseudo-Boolean. Sturgeon *names* kmtotalizer (Morgado et al. 2014) and MiniCard (Liffiton & Maglalang 2012) and neither is held. The closest thing in the corpus is Foged & Horswill's `AtMostConstraint`, which is a propagator for a domain solver, not a CNF encoding — a different artefact solving a different problem. **The plan's `count(vs, lo, hi, weight)` is its largest uncited implementation decision.**

**Pure-Rust SAT/ASP solvers, and solver determinism. Nothing usable.** No survey, no benchmark, nothing on reproducibility of solver output across runs or platforms. The corpus's only Rust-and-solvers document is `10.70675_a795c735zd4fbz4401z8e08z7c353004d1ad` (Denis, *Deductive verification of Rust programs*), whose case study is **Sprout**, *"the first verified SMT solver written in Rust"* — a correctness-verification artefact, not a production solver, and its formalisation *"does not specify data structures or even a deterministic process for execution."* **The plan's §5.1 cannot be answered from the corpus at all.** It is a pure engineering decision and the `-solver-choice.md` document should say so rather than implying a literature basis.

**Determinism of PCG under a solver. Nothing.** Nobody in this corpus has the constraint the plan's §6 has. Every generator here is free to be nondeterministic and most treat that as the point. The nearest anyone comes is Smith & Mateas noting Xorro, *"a tool for sampling answer sets with near-uniform probability"* — which is a randomiser, the opposite of what the plan needs. **"Variety must come from varying the problem per seed, never from solver randomness" is an unsourced invention of this project.** It is a good one, but it is load-bearing and undocumented, and the encoding-order hazard it implies (the plan is right that this is a `sort_total!`-shaped problem) has no prior art to check against.

**Snapping / socket-compatibility as a constraint. Effectively nothing**, confirming the four earlier sweeps. `10.1109_access.2022.3168832` (snappable meshes) is the only document that snaps anything, and it snaps constructively with a pin-count-and-colour match — it does not encode compatibility as a constraint to be solved. There is no corpus support for the plan's `faces: &[Option<Interface>]` seam model beyond the analogy.

**Sturgeon's own timing figures.** Figures 6 and 7 are images and did not survive conversion. Everything in §1.4 above is prose-only. If per-solver head-to-head numbers are ever needed, they are in the PDF at `papers/10/10.1609_aiide.v18i1.21944.pdf` and would have to be read visually — a conversion re-run will not recover them.

**Sandhu et al.'s Table 2.** Missing from the conversion, which is why the success-vs-conflict ambiguity in §4.2 cannot be resolved locally.

---

## 8. Concrete corrections for the plan

1. **§1 / research brief — fix the Karth & Smith DOI.** The constraint-solver-framing paper is `10.1145/3102071.3110566` (FDG '17), held as stem `10.1145_3102071.3110566`. `10.1145/3337722.3341845` is their 2019 discriminative-learning paper.
2. **§1 table — "reachability rules ↔ the closure problem" is not a like-for-like mapping.** Sturgeon's reachability rules encode a **simple path** (in-degree ≤ 1, out-degree ≤ 1); the plan needs a **reachable set** (unbounded degree). Copy Nelson & Smith's `linked/2`, cite `pcgbook-ch08-asp-applications-mazes-levels`, and stop implying Table 5 is the template.
3. **§L3 — "ASP's minimal-model semantics gives this free; that is why Cooper used clingo" is not supported and is contradicted by the paper.** Cooper's encoding admits unfounded cycles on *every* back-end (he draws them in gold in Figure 1(d)) because his support rule has a negative head and so is an integrity constraint, not a founding rule. Foundedness comes from head polarity, not solver family — and a solver-agnostic mid-level API structurally cannot use ASP's semantics even when running on clingo. **The rank encoding is mandatory, not a SAT-only workaround.** This is the single most important correction here.
4. **§0 — restate why WFC-with-extensions was never the alternative.** Not "too slow": Sandhu et al.'s non-local constraints cost ~1 ms and ~1 MB at 100 tiles. The reason is **arity** — every published WFC extension constrains one placement against another placement, and enclosure is a predicate over a set. Cite `10.1145_3102071.3110566` §5.2 for the controlled demonstration that one global constraint defeats greedy-with-global-restart while backtracking resolves it quickly.
5. **§0 — when citing the 60 % figure, scope it.** It is area propagation's conflict rate at a 10×10 map, not the cost of distance-spanning constraints; the paper's own method text calls the same table *successful* generations, so the number is ambiguous by a complement; and Table 2 is missing from the conversion. **The quotable sentence is the other one:** *"Other map sizes were not used because the rate of producing a map drops close to 0 maps/generation when map size is above 10x10."* The plan's target is 12×12.
6. **§4 — 12×12 is small by Sturgeon's standards.** 144 cells sits between Cooper's two smallest evaluated functional grids (128 and 224), both inside a *"about 10s or less by most solvers"* band, in Python, on a 2018 MacBook Pro. Nothing in the corpus suggests a scaling problem at this size. **Watch the maximum, not the median:** Cooper's `void` case is 3.1 s median against 25.2 s maximum on soft pattern constraints, and `repair` — the closest analogue to the plan's L4 soft-constraint-over-existing-artefact shape — costs 10.5 m median and 24.8 m maximum.
7. **§4 gate — calibrate the 20 % non-convergence threshold against Cooper's own sweep**, which had 7/36 hard timeouts at a ten-minute budget and 19/36 cells filled. A gate trip may be a statement about the constraint's tightness rather than about the encoding's correctness.
8. **§5.1 — say out loud that the corpus cannot answer it.** No pure-Rust solver literature, no solver-determinism literature. The one Rust-solver document is a verification thesis about an unoptimised research SMT solver. Add the operational datapoint from `10.5121_ijaia.2023.14302` on clingo's install burden, which bears on the `ALLOWED_DEPS` argument in §6.
9. **§5.2 — flag `count` as the largest uncited decision.** Nothing in the corpus specifies a cardinality encoding. Sturgeon names kmtotalizer and MiniCard; neither is held. Foged & Horswill's `AtMostConstraint` is a domain propagator, not a CNF encoding.
10. **§6 — if an anytime budget is ever added, make it deterministic.** Nelson & Smith's approximate-optimisation trick is *"stopping the solver once it gets close enough or runs for enough time"*; wall-clock cannot appear anywhere that has to hash-match. Budget in conflicts or decisions.
11. **Do not fetch Aloul et al.** It is a shortest-path encoding on an undirected graph with always-usable edges, optimising path length — three differences from what the plan needs, all named by Cooper in one sentence. If it is ever wanted, the author-hosted OA PDF is at `http://www.aloul.net/Papers/faloul_iceee06.pdf` (DOI `10.1109/iceee.2006.251924`, OpenAlex `W2035228826`).
