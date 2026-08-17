# Literature review round 1 — 236 papers acquired, two questions settled

**Date:** 2026-08-11
**Scope:** four-track acquisition sweep (CS + mathematics) into home-still, then novelty search over the
newly indexed material.
**Companions:** `2026-08-10-novelty-options.md`, `2026-08-10-adjacent-math-transfer-audit.md`,
`2026-08-10-meshing-algorithm-catalog-v2.md`

---

## 0. Acquisition

| Track | Acquired | Stubs | Failed |
|---|---:|---:|---:|
| 1 — incremental & dynamic algorithms | 62 | 4 | 24 |
| 2 — GPU-era meshing 2020–2026 | 16 | 2 | 30 |
| 3 — computational topology & DDG | **120** | 0 | 2 |
| 4 — streaming, layout, compression | 38 | 2 | 10 |
| **Total** | **236** | **8** | **66** |

Corpus: **8,277 → 8,740 documents, 253,240 → 268,032 chunks.**

**The download rule, now measured over four runs:** `10.48550/arXiv.*` succeeded **231/233**. Wiley
`10.1111/*` **0/10**. Eurographics `10.2312/*` **0/2**. ACM `10.1145/*` roughly 1 in 14. IEEE `10.1109/*`
~0/4. Mathematics is nearly 100% reachable because arXiv is near-complete for that field; graphics is
mostly locked. Always resolve the arXiv preprint first.

**Operational finding worth acting on:** arXiv's *search* API rate-limits hard and never recovers within a
backoff, but arXiv *downloads* keep working the entire time — different endpoints. The workaround that
carried two tracks: discover via WebSearch restricted to arxiv.org (surfaces `/abs/ID` with titles, so IDs
are resolved rather than guessed), then `paper_download` on `10.48550/arXiv.<id>`.

### ⚠️ The pipeline is partially blocked

`olmocr` is failing on a significant fraction of the new arXiv PDFs — `0 completed pages (failed=27)`,
`workspace/markdown does not exist`. Some recover via the `glm_ocr` fallback; many have not. Two of the four
analysis agents found their entire read list unconverted and had to work from abstracts. **539 documents are
now queued or stuck in `stuck_convert`**, up from 362. `distill_backfill` reports 0 candidates, so
everything that *does* convert is indexed correctly — the bottleneck is conversion, not embedding.

Also: `catalog_backfill_title` has **115 candidates** (the track-3 arXiv papers, which carry DOI + size +
SHA but no title) and times out at 60 s when given a batch of 60. Run it in batches of ~15.

**This blocks the rest of the review.** Everything below rests on the fraction that converted.

---

## 1. Integer Coordinates → DOES NOT TRANSFER

`10.48550_arXiv.2106.00220` — Gillespie, Sharp & Crane, *Integer Coordinates for Intrinsic Geometry
Processing*. The hypothesis was that this made the same move as Subgrid Marching Tetrahedra, earlier, and
might carry more machinery with it.

It doesn't, and the reason is structural. Integer Coordinates is **a correspondence tracker between two
triangulations of the same 2-manifold**, not a surface encoding. Its integer `n : E¹ → ℤ` counts crossings
of `T¹` edges *by edges of a second triangulation `T⁰`*. Its "roundabouts" `r_aj` exist solely to
disambiguate *which edge of `T⁰`* a traced curve is. Its decoder works only because the encoded curves are
**geodesics that terminate at vertices**.

In voxel meshing: there is no `T⁰`. The isosurface is codimension-1 in a 3-manifold, not a retriangulation
of it. Subgrid MT explicitly requires every intersection to be **strictly interior to a grid edge**, so
`V⁰ ∩ V¹ = ∅` and the roundabout array is empty. Isosurface loops on a tet boundary are closed, so
`TraceFrom` has **no termination condition**. Three of the four fields have no referent — not hard to
compute, undefined.

**And the generality runs the other way.** Subgrid MT's decoder is strictly more general: it assigns valid
intersection-free geometry to *arbitrary* edge coordinates, including `e = (1,3,0,3,3,0)`, which "cannot be
encoded by any normal or almost normal surface" (Subgrid MT p.5). It also augments edge coordinates with
intersection locations and normals, "which are not part of classic normal surface theory."

### What it does have that Subgrid MT lacks

**Normal-coordinate arithmetic under mesh mutation** — closed-form integer update rules for flip, split,
remove, and vertex motion. Subgrid MT has none of this because its grid is fixed. If you ever want an
adaptive tet grid, exact integer maintenance under flips is the thing to steal — but Integer Coordinates
supplies it for *curves on a surface* (1 integer per edge). The voxel analogue needs normal *surface*
coordinates in a 3-manifold (7 per tet) under 3D Pachner moves, which this paper does not contain. Research
direction, not a transfer.

**Cross-edit attribute transfer — the one genuine gap.** §3.8 builds a common subdivision from `n` alone;
§4.4 transfers a function between triangulations as the L²-nearest solution,
`‖f − f̂‖² = (P₁f − P₀f̂)ᵀ M_S (P₁f − P₀f̂)`, prefactorable for many transfers. That solves a problem
Subgrid MT doesn't touch: **after a CSG edit re-meshes a region, how do you move vertex colours, UVs,
material IDs and paint onto the new mesh in a principled way?** Every sculpting tool ships nearest-point
copy for this.

**The cheap experiment, and why it's cheap:** both meshes are cut by the *same* tet grid, and every vertex
of both already carries its provenance as (tet, edge, parameter t) from the `e`-vectors — so **the shared
grid is the common refinement**, and the expensive part of the paper (tracing curves to find `T⁰∩T¹`) is
unnecessary. Paint a synthetic signal with known analytic form on `M₀`, apply a brush, transfer to `M₁`
three ways (nearest-point / L²-projection / ground truth), measure error.

Falsifiable prediction: across a CSG edit `M₀` and `M₁` are **identical outside the brush footprint**, so
the L² machinery can only pay in the thin band the brush touched. If the improvement is confined there and
small, the verdict hardens to "does not transfer, and the one plausible exception doesn't pay either."
One afternoon, one model, decisive.

Warning to log while you're there: the paper's own blowup numbers are common-subdivision `|V|` mean **20×**,
95th percentile **45×**. If that ratio shows up per-edit, the transfer structure costs more than the mesh.

---

## 2. "Manifold or intersection-free, pick one" is FALSE — and was already false in 2010

This is the strongest result of the round.

**The folklore is an accurate 2015 trip report about two specific algorithms, promoted into a law it never
was.** The Dreams slides are a ping-pong between two patches to Dual Contouring: p.47 applies Ju & Udeshi's
intersection-free contouring, p.48 applies Manifold DC, p.49 reports self-intersection returning. That is
not an impossibility argument.

**And the cycle had already been broken five years earlier, by a co-author of Manifold DC.** Manson &
Schaefer, *Isosurfaces Over Simplicial Partitions of Multiresolution Grids*
(`10.1111_j.1467-8659.2009.01607.x`, 2010), abstract: *"We provide a simple method that extracts an
isosurface that is **manifold and intersection-free** from a function over an arbitrary octree."*

The mechanism is exactly the clamp, verbatim from the paper:

> "As long as the dual vertices lie within their corresponding cells, this process cannot produce any
> inverted tetrahedra and creates a partition of space... Since our decomposition is one-to-one, our surface
> cannot intersect itself. Each edge in our surface will also be contained by exactly two polygons since
> tetrahedra share a common triangle face with adjacent tetrahedra. Finally, since vertices of the
> isosurface cannot lie at the end-points of the edges of the tetrahedra, our isosurface is guaranteed to be
> manifold and intersection-free."

**The empirical kill shot.** Occupancy-Based Dual Contouring (`10.48550_arXiv.2409.13418`, 2024), §6.3,
measured over 3,150 generated meshes:

> "all 1,500 ODC output meshes in the SALAD experiment are free of self-intersections and have manifold
> properties. In the 3DShape2VecSet experiment, all 1,650 ODC output meshes are manifold, with only one
> exhibiting self-intersection, a significant improvement over our backbone MDC, **which has 100% of meshes
> with self-intersections**."

MDC fails on 100% of meshes across two independent datasets. A rule that cannot avoid a failure fails
everywhere — which is the signature of a *rule* property, and precisely why the folklore felt like a law.

Two independent modern confirmations that this is a design axis: **TetWeave** (2025) lists 2-manifold and
intersection-free as separately achievable guarantees, with *"Contrary to FlexiCubes, our method is
guaranteed free from self-intersections."* **Subgrid MT** (2026) states it as an engineering knob:
*"in situations where an intersection-free guarantee is required, one could still adopt conservative
placement strategies, or revert to primal reconstruction for cells with mesh self-intersections."*

### The three questions practitioners conflate

| Question | Input | Complexity |
|---|---|---|
| **Detect in a given mesh** — do any two non-adjacent triangles with *these* coordinates intersect? | connectivity **+ coordinates** | **In P.** BVH broad phase + exact predicates. Near-linear in practice. |
| **Does a rule guarantee it** — does extractor R produce intersection-free manifold output for *every* field? | the algorithm | **Not a search.** A proof obligation discharged once at design time; runtime cost is an O(1) clamp per vertex. |
| **Does an abstract complex admit an embedding** — topological, in R³ | connectivity **only** | **Decidable** (1402.0815) but **NP-hard** (1708.07734), with no matching upper bound and no claimed complexity bound at all |
| — same, but *geometric* (straight-line) | connectivity **only** | **∃R-complete** for all d ≥ 3, k ∈ {d−1, d} (2108.02585) |

The conflation is between the first and the third. A mesh from a contourer is **never** an instance of the
third question, because it arrives with coordinates already chosen. "Could this mesh be repositioned to be
intersection-free?" is the ∃R-complete one — throw away coordinates, keep connectivity. "Does this mesh
self-intersect as positioned?" is the cheap one. Same triangles, different questions, and the complexity gap
between them is the whole content of the confusion.

Note also that 1402.0815's decision procedure takes a **triangulated 3-manifold** measured in tetrahedra,
and the only quantitative handle in its abstract is that the meridian length is bounded by *"a computable
function of the number of tetrahedra."* No polynomial, elementary, or primitive-recursive bound is asserted.
**No size of complex is checkable at chunk scale**, and that isn't a hedge — it's a question you must never
ask at runtime.

### Say this instead

> Detecting whether a given mesh self-intersects is cheap — a geometric predicate over coordinates you
> already have, near-linear with a BVH. Guaranteeing an extractor never produces one is a design property
> you prove once about the rule, bought by keeping every generated vertex inside a cell of a genuine
> partition of space. Deciding whether a bare abstract complex admits *any* embedding is a different
> problem — NP-hard topologically, ∃R-complete geometrically. My mesher never asks that third question,
> because my mesher has coordinates.

Or compressed for an argument in a channel:

> **Dual Contouring and its manifold patch can't give you both. That's a property of those rules, not of
> meshes. Partition-based extractors give you both, and have since 2010.**

Cost of the guarantee, from Manson & Schaefer's own tables: ~50% over DMC, ~2× DC (2.58 s vs 1.35 s,
armadillo at depth 8) — partly repaid because their extra edge and face samples let them run one octree
level shallower for equal sampling density.

### The experiment, and the metric fix

**Report λ, not p.** ODC reports the *fraction of meshes with ≥1 self-intersection*. For a chunked engine
that statistic is actively misleading — it saturates with chunk size, since `p = 1 − e^{−λT}` for
intersection rate λ per triangle and chunk triangle count T. A large enough chunk shows p ≈ 1 for any λ > 0.
Log **intersecting pairs per 1,000 triangles** alongside it.

**The one decisive intervention, before anything else:** clamp the QEF minimizer strictly inside its own
cell, scaled by (1−ε), exactly as Manson & Schaefer specify. Re-measure λ.

- **λ → 0** — your failures were unclamped placement. Fixed, at some sharp-feature cost, and the folklore
  never applied to you.
- **λ unchanged** — your failures are in *connectivity*: multiple surface sheets through one cell, which is
  DC's actual structural defect. The fix is architectural (partition-based extractor), not another patch.

**Bucket every detected pair from day one** as intra-cell / inter-cell within chunk / cross-chunk-seam.
Cross-seam intersections are a stitching bug with nothing to do with the DC literature, and they will
otherwise dominate λ and send you chasing a contouring problem you don't have. None of the source papers
chunk, so none of them discuss this bucket.

---

## 3. Notable acquisitions not yet analysed

- **`10.48550_arXiv.2505.02017` — Aokana.** GPU-driven voxel rendering for open-world games: sparse voxel
  DAG + explicit LOD + streaming, "tens of billions of voxels," memory reduced "up to ninefold." The closest
  published *system* to the engineering question. Not yet read.
- **The batch-dynamic trees line** — `2002.05129` (change propagation with a computation-distance metric),
  `2306.08786` (**deterministic** and work-efficient, where all prior results were randomized-in-expectation),
  `2601.10706`, `2506.16477`. This is the incremental-hierarchy problem under a different name, with a mature
  literature. `2501.07745` (DynHAC) identifies *which part of a dendrogram* must be rebuilt so the result is
  identical to from-scratch — the exact "from-scratch consistency under localized repair" framing.
- **`10.48550_arXiv.2601.05347` — Parallel Dynamic Spatial Indexes.** Batch updates to Orth-trees and
  R-tree/BVH with work/span bounds and an implementation. Your problem statement in database form.
- **CALM** (`1901.01930`, `2602.09435`) plus the local-certification line (`1011.2152`, `1311.7229`,
  `1910.12747`) — monotone iff coordination-free, stated as an equivalence. This is the Nanite
  "same input ⇒ same output" question with a theorem attached.
- **Bytes-per-primitive figures for comparison with Nanite's 5.6 B/triangle:** Fusy/Poulalhon/Schaeffer hit
  the information-theoretic floor at "2+o(1) bits per edge" (≈3 bits/triangle for connectivity);
  Choe/Kim/Lee report "about 11 bits for connectivity and 21 bits for geometry with 12-bit quantization"
  ≈ 2 bytes/triangle — below Nanite, but without cluster/LOD metadata.

## 4. Could not acquire, and it matters

- **Wardetzky et al., "Discrete Laplace operators: No free lunch."** No OA copy under any DOI; no arXiv
  preprint exists. This is the canonical "these discrete-Laplacian properties are mutually unsatisfiable"
  theorem — structurally the same shape as the meshing tradeoffs, and the best available model for how to
  state one rigorously. Worth manual retrieval.
- **Scholz, Bender & Dachsbacher (`10.1111/cgf.12462`)** — closest published work to "editable + LOD
  isosurface." Wiley, hard-blocked, 8/8 CGF DOIs failed.
- **The entire Isenburg/Lindstrom streaming-meshes line**, the Maglo mesh-compression survey, and
  Sander/Nehab/Barczak triangle reordering.
- **GPU work graphs** is a genuine literature hole, not a search failure — the only peer-reviewed treatments
  are HPG 2024, a SIGGRAPH 2025 course, and an ISCA-adjacent paper, all AMD-affiliated and closed.

## 5. Next

1. **Unblock conversion.** 539 stuck; olmocr failing on many arXiv PDFs with `glm_ocr` recovering only
   some. This gates everything else.
2. `catalog_backfill_title` in batches of ~15 (115 candidates).
3. Re-run the novelty search once the queue drains — tracks 1 and 4 are essentially unanalysed.
4. Read Aokana and the batch-dynamic-trees line against the incremental-LOD question.
