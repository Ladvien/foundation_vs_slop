# Novelty options for game meshing — six moves, three executed

**Date:** 2026-08-10
**Method:** `paper_search` meta-search (Crossref / OpenAlex / Semantic Scholar / CORE) against the live
literature, not just the local corpus. Three of six novelty moves run end to end.
**Companions:** `2026-08-10-meshing-algorithm-catalog-v2.md`,
`2026-08-10-adjacent-math-transfer-audit.md`

---

## 0. The six moves

| Move | What it is | Cost | Risk |
|---|---|---|---|
| **A. Measure the unmeasured** | Publish a number a whole field failed to report | weeks | low — you either get the number or you don't |
| **B. Cross-family composition** | Combine components nobody has crossed | months | high — needs the falsifier first |
| **C. Adjacent-math import** | Lift a defect to an invariant, find the named structure | days per lift | high — most die at the transfer audit |
| **D. Adjacent-*engineering* import** | Steal a working solution from another discipline | days per lift | medium |
| **E. Constraint inversion** | Drop an assumption everyone accepts | days | medium |
| **F. Regime shift** | The hardware moved; the tradeoff flipped | weeks | low |

Run so far: **C** (3 lifts, previous doc), **A**, **D** (2 lifts). Not yet run: **B**, **E**, **F**, and
two remaining C lifts.

---

## 1. Move A — what the field never measured

### Two gaps are CLOSED. One of them inverts a claim I made in catalog v2.

**Marching Cubes cost inside a real-time volumetric loop — measured, and it's the dominant cost.**
Dong, Shi, Tang, Wang & Zha, *"An Efficient Volumetric Mesh Representation for Real-Time Scene
Reconstruction Using Spatial Hashing"*, ICRA 2018, **DOI 10.1109/ICRA.2018.8463157**. Incremental meshing
isolated as its own stage inside a voxel-hashing TSDF pipeline, i7-6700HQ + GTX 1070M, 3 cm resolution:

| Dataset | Meshing (ms) | All stages (ms) |
|---|---:|---:|
| ICL/lv1 | 6.06 | 7.20 |
| TUM/household | 10.94 | 12.21 |
| Zhou/lounge | 4.03 | 5.05 |

**Meshing is 76–90% of total pipeline time.** Catalog v2 said "nobody in the real-time volumetric line
reports a polygonization cost — they all raycast." The first half was wrong; the second half is now
*explained* by the first. They raycast **because** meshing dominates.

Corroborated by **nvblox** (Millane et al., DOI 10.48550/arXiv.2311.00626): mesh stage 1.6 ms (RTX 3090Ti)
/ 4.0 ms (RTX 3000 Mobile) / 12.3 ms (Jetson Xavier AGX), meshing dirty blocks every 4 frames.

**Incremental navmesh rebuild — measured.** van Toll, Cook & Geraerts, *"A navigation mesh for dynamic
environments"*, CAVW 2012, **DOI 10.1002/cav.1468**. Core2 Duo 3.0 GHz, single core: point insertion
0.2–0.6 ms; polygon insertion 1.3–2.4 ms; polygon deletion 1.2–5.4 ms; moving obstacle average 0.29–1.09 ms;
versus 9–22 ms for full GPU reconstruction. Caveat: 2D/2.5D medial-axis navmesh, not voxelized Recast.
A *comparative* update-latency study across navmesh types is still open — DOI 10.1145/2994258.2994262
explicitly declines to benchmark it.

### Three gaps survive

**Re-mesh latency after a local edit — survives, narrower than stated.** The closest published thing is
Wegen, Döllner & Trapp, *"Interactive Editing of Voxel-Based Signed Distance Fields"*, J. WSCG 2022,
**DOI 10.24132/jwscg.2022.9**. Ryzen 5 5600X + RTX 3090: CSG unification **3 / 17 / 146 / 903 ms** at
1.5M / 10.8M / 86M / 697M voxels; SDF recalculation **24 / 204 / 1024 / 8362 ms**. That gives you the
*edit* half at modern-GPU scale — **but they raymarch and never extract a mesh.** Nobody isolates
ms-to-regenerate-one-chunk's-triangle-buffer as a function of chunk size × brush size × algorithm.

⚠️ One paper could close this and Gap 2 and could not be retrieved: Chen, Hu, Chu & Chen, *"A Real-Time
Sculpting and Terrain Generation System for Interactive Content Creation"*, IEEE Access 9 (2021),
**DOI 10.1109/ACCESS.2021.3105417**. Xplore returned 418, no OA mirror. **Check this before publishing any
negative claim.**

**Per-chunk memory accounting — survives strongly.** Queries diverged into cognitive-psychology "chunking"
and storage dedup, the signature of a genuinely absent literature. Closest: Dong 2018's "56 bytes per unit…
50000 blocks, 1.8M vertices, 3.5M triangles ≈ 1.6 GB; minimized to 24 bytes → around 700 MB." No published
instance of the paired quantity — voxel payload bytes vs resident mesh bytes, per chunk, under palette/RLE
variants.

**Mesh extraction as a fraction of shipping-game frame time — survives, but unpublishable by you.**
Requires a shipping title's profiler data. Academic work is aggregate FPS comparisons only.

---

## 2. Move D — mortar / non-conforming FEM

### Verdict: DOES NOT TRANSFER, and the reason is worth more than a transfer would have been

Take the shared face Γ, traces `u = f_h|_Γ` (fine) and `v = f_H|_Γ` (coarse), isovalue σ. By the implicit
function theorem the gap between the two extracted curves at a point is

```
d(x) ≈ |u(x) − v(x)| / |∇_Γ v(x)|
```

**The crack is the jump divided by the tangential gradient.** Mortar bounds the numerator in L² or
H^(−1/2). Two failures follow. L² control isn't L^∞ control — a small mean-square jump permits a large
jump concentrated on a few faces, which *is* a crack. And `d(x)` blows up as `|∇_Γ f| → 0`, i.e. exactly
where the isosurface is tangent to the interface plane — which is Lengyel's own documented worst artifact
(corpus copy, p.3: *"a large hole appears where the terrain surface is nearly tangent to the boundary plane
between the blocks."*).

**The elegant part.** Specializing the full mortar apparatus to a dyadic ℓ/ℓ+1 face collapses to: *the
coarse side's face values are the L² projection of the fine side's.* That's a geometric-multigrid
restriction operator with mortar-blessed weights — and it still leaves a crack, because projection ≠
identity. Go one step further, replace projection with **injection** (coarse reads the fine face samples),
and you have re-derived **Transvoxel**. Lengyel picked master/slave correctly and chose the right operator,
constructively, without the theory.

Three further breaks: mortar constrains the stationarity condition of an *energy functional* and there
isn't one here (the field is already globally defined; both chunks merely sample it); octree LOD gives
*dyadically nested* trace spaces so the constraint is satisfiable by injection and the multiplier is
unnecessary; and the contour position is a **nonlinear pointwise functional** of the trace while mortar's
constraint is linear.

Nitsche is the *worse* fit despite being easier — it penalizes rather than enforces, leaving a residual
jump, and a sub-pixel crack still flashes skybox.

### Five things that survive anyway

1. **A Hausdorff bound Transvoxel doesn't carry.** Strang's second lemma gives seam-adjacent surface
   displacement ≈ `C·H²‖D²f‖_∞ / |∇f|`. Ten lines of derivation.
2. **Master/slave asymmetry as a design axiom.** The criterion is unambiguous: *constrain the space that is
   contained in the other.* Fine-master is provably right; skirts and locked-boundaries are coarse-master
   and cost an order at the seam.
3. **The mesh-tying patch test as a shipping unit test.** Set the field to an exact plane representable in
   both trace spaces, run every seam rule, check reproduction to machine precision. Skirts fail;
   locked-boundary passes at coarse order; Transvoxel passes on Γ. **Nobody in game meshing runs this.**
   Costs an afternoon and is diagnostic.
4. **Dual/biorthogonal multipliers for *attribute* transfer across seams** — normals, material IDs, AO,
   vertex colours. Those are genuine fields where C⁰ suffices and nobody perceives an L² error as a crack.
   This is the one sub-problem where the apparatus fits unmodified, and it's currently done by ad-hoc
   averaging.
5. **Common refinement wakes the theory back up** the moment your LOD stops being a clean octree —
   non-power-of-two ratios, independently authored volumes, clipmaps. Jiao & Heath,
   **DOI 10.1002/nme.1147**, is then the right tool with real theory behind it.

Plus a float-level finding below the resolution of the continuum theory: even with bitwise-identical trace
functions, `(σ−f_A)/(f_B−f_A)` and `1−(σ−f_B)/(f_A−f_B)` are not bitwise equal. **Canonicalize edge
traversal by lexicographic integer grid coordinates** so both sides evaluate the identical expression.

---

## 3. Move D2 — incremental computation. This is where the real opening is.

### Isosurface extraction is in the best complexity class, and the cost is somewhere else

Marching cubes / dual contouring is **1-local**: output at cell *c* depends only on *c*'s 8 corners.
Fan-in 8, fan-out 1. Under Ramalingam & Reps (DOI 10.1016/0304-3975(95)00079-8), Fan/Hu/Tian
(DOI 10.1145/3035918.3035944) and Berkholz et al. (DOI 10.1145/3034786.3034789) it is **bounded,
localizable, and q-hierarchical-shaped**. Ley-Wild/Acar/Fluet's trace-distance bound
(DOI 10.1145/1594834.1480907) then says update cost is provably Θ(|δ|).

**So the 30k-instead-of-2k cost is not caused by meshing.** It's caused by three things layered on top,
all of which are global-order aggregations: index-buffer compaction, cross-seam vertex welding, and the LOD
simplification chain. That reframing is the main analytical result.

### The gap: incremental LOD-hierarchy repair. Measured thin.

- **Puppo's Multi-Triangulation** (DOI 10.1016/S0925-7721(98)00029-7, 1998) models a multiresolution mesh
  as a poset of local update fragments with dependencies — a strictly *more general* formulation than
  Nanite's cluster DAG, 23 years earlier. Published operations are **extraction only.** Editing the base
  data and repairing the DAG is not treated.
- **Exactly one paper** does dynamic update of a multiresolution terrain: Rocca, Panozzo & Puppo,
  *"Patchwork Terrains"*, DOI 10.1007/978-3-642-38241-3_4. Essentially zero citations. That count *is* the
  measurement of how thin this is.
- **Everyone incrementalizes the cut, never the hierarchy.** Hoppe's VDPM and ROAM both maintain an active
  front across frames — over a precomputed, immutable vertex hierarchy.
- **Nanite explicitly doesn't do it.** Its grouping is a **global METIS partition** of the cluster adjacency
  graph, chosen against the original mesh to minimize locked edges. A local edit invalidates the
  *optimality* of that partition, not just the contents of one group. That's why nobody has an incremental
  version.

### Two existence proofs that it isn't blocked

**SVDAG in-place editing** — Careil, Billeter & Eisemann, **DOI 10.1111/cgf.13916**. An SVDAG has the
ancestor problem in its most extreme form: node identity is a hash of the entire subtree, so flipping one
leaf changes every ancestor's identity to the root and the DAG must be re-deduplicated. They do it
**interactively, on GPU, in place, without de/re-compression**. Extended by DOI 10.1111/cgf.14757 and
DOI 10.1111/cgf.70292.

**Concurrent Binary Trees** — Dupuy, DOI 10.1145/3406186. Hierarchy as a bitfield plus its sum-reduction in
a flat heap; ancestor work is repairing an **associative reduction**, trivially parallel and pointerless.
And directly on target: Scholz, Bender & Dachsbacher, **DOI 10.1111/cgf.12462** — LEB-based adaptive
isosurface extraction with **no stitching between LOD regions, no preprocessing, explicit dynamic-data
support.** This is the closest published thing to "editable + LOD isosurface" and it works precisely
because the hierarchy is implicit in the *domain*, not derived from the *mesh*.

### The unmeasured failure mode

**BVH refit-degradation transfers, and is strictly worse — for three reasons, only one of which has a BVH
analogue.**

1. *Structural drift* — the direct analogue. Lauterbach et al. (DOI 10.1109/RT.2006.280213) give the
   degradation metric `d` normalized by interior-node count, with **rebuild when d > 0.4**. The LOD
   analogue: METIS grouping was optimal for the original mesh; after a dig, boundary edges are locked in
   the wrong places and the reduction ratio per group monotonically worsens. **Nobody has written down the
   analogue of `d`.**
2. *Error accumulation* — **no BVH analogue at all.** AABB refit is *exact*: parent = union of children.
   Simplification has no such property. Re-simplify a repaired parent from already-simplified children and
   QEM error compounds multiplicatively along the chain. Nanite escapes this because the chain is built
   once, offline. Under repeated repair it's re-walked hundreds of times per chunk lifetime.
   **No published analysis.**
3. *Determinism drift* — a correctness bug. Refit is an associative min/max reduction; QEM edge collapse is
   priority-queue order with float tie-breaks. Two clients with the same edits in different orders converge
   to different meshes.

And one datum arguing against incrementalizing the collider at all: Benthin & Peters
(**DOI 10.1111/cgf.14868**) rebuild a *complete* BVH over LOD-selected clusters **every frame** at >74% of
peak memory bandwidth. Just rebuild the collider.

---

## 4. The ranked menu

| # | Candidate | Novelty evidence | Cost | Why it matters |
|---|---|---|---|---|
| **1** | **Incremental LOD-hierarchy repair under local edits** | One near-zero-citation paper; Nanite explicitly doesn't; two existence proofs it's tractable | E2 = 1 afternoon for first data; full result = months | The crux for editable + LOD. Nobody owns it. |
| **2** | **Degradation curve of a repeatedly-repaired simplification hierarchy** | No published analysis; the error-accumulation mode has no BVH counterpart | 1 afternoon (offline script) | Tells you the compaction period, or that you don't need one |
| **3** | **Re-mesh latency curve** | Gap survives; closest published work never extracts a mesh | 2–3 weeks | The number the whole field is missing |
| **4** | **Voxel-vs-mesh byte budget per chunk** | No published instance of the paired quantity | 1 week on top of #3 | Decides cache-vs-recompute, quantitatively |
| **5** | **Seam displacement bound via Strang's second lemma** | Transvoxel carries no bound | 10 lines + writeup | Gives an ad-hoc method its theory |
| **6** | **Equivariant vertex rule** (previous doc) | Unoccupied niche | half day | Kills a pop class; branch-free GPU kernel |
| **7** | **Mesh-tying patch test for seam rules** | Nobody in game meshing runs it | 1 afternoon | Separates consistent seam rules from merely plausible ones |

### Free wins, do these regardless

- **Content-addressed early cutoff.** Hash each cluster slab in canonical cell order; unchanged hash ⇒
  don't dirty the collider, don't dirty the LOD parent. Digging into solid rock or empty air produces
  *zero* dirty clusters. Price: one hash.
- **Canonicalize edge traversal** by lexicographic integer coordinates — fixes a bitwise crack class the
  continuum theory can't see.
- **Quantize boundary vertices** to a shared level-independent integer lattice — without exact
  representability no seam certificate can exist.
- **Assert `∂₂[M] = 0`** by directed-edge hashing. O(#triangles).
- **Derive LOD from the field, not the mesh.** Then LOD *k* is a pure function of the field at level *k*:
  no simplification chain to keep consistent, no QEM order dependence, no error accumulation, and ancestor
  repair degenerates to a **mip filter — an associative reduction**, the shape CBT proves GPUs like. You
  trade Nanite-quality organic simplification for a deterministic, cheaply-repairable approximation. Given
  that nobody has made the mesh-space path work incrementally, this is the trade to take.

### The two cheapest experiments, in order

**E1 — measure the early-cutoff hit rate before building anything.** Instrument the existing mesher. Per
brush stroke log: voxels touched, cells whose MC case index actually changed, clusters whose output hash
changed. If the third is 5–15% of clusters, the early cutoff pays for itself immediately and you know your
speedup ceiling before writing the hard parts. If cells change but *all* clusters change, your clustering
is too coarse or the index buffer is entangling everything — and that's a data-layout bug, not an
algorithms bug. Cost: a hash and two counters.

**E2 — simulate the degradation curve offline.** One chunk, 1000 recorded brush strokes. After each,
compute the LOD chain two ways: full rebuild from the field, versus incremental repair from
already-simplified children. Plot Hausdorff error, triangle count, and a Lauterbach-style `d` against N.
Flat at N=1000 ⇒ skip the compaction machinery entirely. Blows up at N=50 ⇒ you've measured your compaction
period. Either way you've answered a question nobody has published, in hours.

---

## 5. Corrections to earlier docs

- **Catalog v2 §6 is wrong.** "Nobody in the real-time volumetric line reports a Marching Cubes frame cost"
  — Dong et al. 2018 does, and meshing is 76–90% of that pipeline.
- **Catalog v2 §13** listed incremental navmesh rebuild timing as unmeasured — van Toll et al. 2012
  measured it.
- **Catalog v2 §13** listed Transvoxel as absent — it's in the corpus, paper and dissertation.
- The corpus was missing Scholz et al. (DOI 10.1111/cgf.12462), which is the closest published work to the
  thing you're trying to build. Worth downloading with the SVDAG editing line.

## 6. Not yet run

- **Move B** — cross-family composition, systematically. Needs the falsifier harness first.
- **Move E** — constraint inversion. Enumerate the load-bearing assumptions across the corpus and check
  which have been dropped in the literature and which haven't.
- **Move F** — regime shift. Every number in the corpus is 2005–2013 silicon; the dense-vs-sparse TSDF
  tradeoff may have flipped on 24 GB cards, and mesh shaders / work graphs postdate most of it.
- **Move C** — two remaining lifts: embedding-vs-immersion obstruction (the "manifold or intersection-free,
  pick one" folklore), and matroids for coordination-free hierarchical cuts.
