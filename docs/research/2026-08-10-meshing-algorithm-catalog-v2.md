# Meshing algorithms — catalog v2

**Date:** 2026-08-10
**Supersedes:** `2026-08-10-meshing-algorithm-catalog.md` (v1, same day)
**What changed:** v1 was written against 38 corpus papers, several of them read only second-hand. Since
then 42 papers were acquired and 20 of the most load-bearing were read as primary sources. Four of v1's
conclusions were wrong. This version replaces them.
**Method:** 7 parallel reading agents over the newly acquired primary sources, every figure quoted with a
page or section reference, then merged into v1's family structure.
**Companions:** `2026-08-10-meshing-library-target.md` (bibliography + acquisition status),
`2026-08-10-home-still-curation-agent-prompt.md` (corpus repair).

---

## 0. What v1 got wrong

**1. Surface Nets does not round sharp features. It sharpens them.** This is the most widely repeated
piece of folklore in voxel meshing and Gibson's paper says the opposite, repeatedly:

> "After relaxation, curved surfaces are relatively smooth, **corners are sharp**, and thin structures are
> preserved." … "the net energy decreases quickly to a minimum and then increases slowly and asymptotically
> to a slightly higher level. At the minimum energy level, the surface appears to be smoothest, **but
> corners become sharper as the energy increases to the final level**."

The mechanism is the box constraint, not gradients — each node is clamped inside its original surface cube,
so corners cannot round off. Footnote 1 notes that a *curvature-based* relaxation "would produce smoother
surfaces and **with less sharp corners than the method used here**" — the rounding everyone attributes to
Surface Nets is a property of a variant Gibson explicitly declined to use. The correct caveat is narrower:
you get sharp *grid-aligned, voxel-scale* corners, and you cannot get a crease at an arbitrary angle,
because there is no normal information anywhere in the algorithm.

**2. Dual Contouring is not the biggest gap — Transvoxel is.** v1 listed DC as absent; it was in the corpus
all along with zero vector chunks (see the curation doc). Having now read it: DC's minimal-edge rule needs
**no crack patching and no restricted octree**, which reverses the usual complaint about DC and LOD.

**3. "Nothing in this corpus is fast" was wrong.** The real-time volumetric line (§6) has hard per-frame
budgets: **2 ms to rewrite an entire 512³ TSDF** (KinectFusion), **21.8 ms full pipeline at ~46 fps**
(voxel hashing). v1 missed this because those papers weren't in the corpus yet. This is now the most
important section in the document for a destructible-terrain engine.

**4. The generative-topology story is weaker than v1 implied, and now has a citation.** v1 quoted QUADify's
sliver percentages. The stronger evidence is that **no paper in the autoregressive line reports a manifold
percentage, watertight percentage, quad ratio, or singularity count** — "topology quality" is measured as
FID on rendered *wireframe images*, or by 30–50 person preference votes. MeshAnything V2's own limitations
section reads: *"the accuracy of MeshAnything V2 is still insufficient for industrial applications."*

---

## 1. Decision table

| If you need… | Use | Cost |
|---|---|---|
| Smooth voxel terrain, cheapest possible | **Surface Nets** | 1 vertex/surface cube; non-manifold pinches at ambiguous cubes; iteration-count-dependent (needs ghost halo for chunk agreement) |
| Sharp features from CSG brushes | **Dual Contouring** | Hermite data (~3× voxel payload); non-manifold at ambiguous configs; QEF must use QR not `AᵀA` |
| Sharp features **and** guaranteed crack-free LOD | **Cubical Marching Squares** | Hermite data; fan triangulation makes slivers; branchy, doesn't GPU well |
| Blocky/Minecraft-style | **Greedy meshing** | 2.76× fewer tris than face culling; no LOD, no seam story |
| Provable manifold + intersection-free | **Subgrid MT (primal)** | Needs all-roots-along-edge finding; primal path has no sharp features |
| Editable volume backing store | **Voxel hashing** | <300 MB for a 30 m scene @8 mm; no LOD, flat by design |
| Static world LOD at scale | **Nanite-style cluster DAG** / **CBT** | Bake step; Nanite needs full-res input |
| Collision proxies | **CoACD** | 1–4 min/part offline; needs 2-manifold solid input |
| Photogrammetry ingest | **Screened Poisson** + denoise pre-pass | 14–20 s at depth 8–9 |
| Differentiable extraction | **FlexiCubes** (default) or **TetWeave** (guarantees) | See §10 |

---

## 2. The one benchmark the corpus actually ran

`10.33842_2313-125x-2023-25-11-21` — UE5, 64 chunks, 32-voxel chunks, 3D Perlin, seed 1337, freq 0.03.

| Mesher | Triangles | Vertices | Stall (1 thread) | With MT |
|---|---:|---:|---:|---:|
| Cubic, no culling | 915,588 | 610,392 | 10,869 ms | 8,843 ms |
| Cubic + face culling | 18,492 | 12,328 | 1,842 ms | 715 ms |
| **Greedy meshing** | **6,690** | **4,460** | 2,357 ms | **461 ms** |
| Surface Nets | 18,492 | 12,328 | 2,050 ms | 811 ms |
| Marching Cubes | 17,808 | 17,808 | 1,824 ms | 1,080 ms |

Greedy is slowest single-threaded, fastest multithreaded (5.1× from threading vs MC's 1.7×). MC shows zero
vertex sharing — the naive-from-the-paper failure. Surface Nets matching face-culled cubes exactly is
structural (one quad per sign-changing edge = exposed face count); the identical *vertex* count means their
implementation didn't weld.

---

## 3. Isosurface extraction on regular grids

Unchanged from v1 in structure. Three additions from new primary sources.

**Custodio et al. 2013 (`10.1016_j.cag.2013.04.004`) — MC33 emits non-manifold edges.** Four defects found
in Chernyaev's MC33 and Lewiner's implementation, verified against a topology-verification framework:

- Frequency in real data: *"on average, **one case of non-manifold edges was found per 10⁷ evaluated
  voxels**"* (p.10). On the skull dataset, *"this problem appeared six times in total for 50 distinct
  isosurfaces."* In random 5×5×5 fields, *"only once in 10000."*
- Root cause (p.5–6): Chernyaev's interior test tracks a **quadratic** `at²+bt+c`, but the true saddle
  trajectory is **hyperbolic with an asymptote** and can change sign more often — so case 13.5.2 is
  misread as 13.5.1. Exact counterexample values are printed in Appendix A, p.12.
- Lewiner's implementation is **missing the disambiguation step for cases 10 and 12** entirely (p.7).
- Fix cost: subdivision adds `kn²` voxels with `k = O(1)`, so *"the asymptotic size of the dataset does not
  change"* (p.9).
- The load-bearing practical finding (p.9): *"for real-world datasets, **the vast majority of Marching
  Cubes cases match the non-ambiguous configurations, namely, 1, 2, 5, 8, and 9**."*

The definition worth internalizing (p.1): **topologically consistent** = crack-free PL manifold;
**topologically correct** = homeomorphic to the trilinear interpolant. *"Although there are many
topologically consistent MC-based techniques, only a handful are topologically correct."* **A game needs
consistency, not correctness.** Skip the interior test; spend the budget on chunk seams. But do add a
manifold validator to your chunk bake — 13 non-manifold edges per 512³ world load will silently corrupt a
half-edge simplifier or a collider baker.

**Neural Marching Cubes (`10.1145_3478513.3480518`) — do not ship this.** 44 added vertices per cube (24
face + 8 interior on top of 12 edge); 2¹⁵ codes collapse to 37 cases. **85,544 triangles at 64³ against
MC33's 10,954 — a 7.8× tax.** Self-intersections: *"**32.7%** of the meshes … contain self-intersections,
but they involve only **0.0086%** of the triangles … about **7.39 triangles** … per shape"* (p.13). Its
sharp-edge recovery is genuinely the best in its comparison set (EF1 0.758 vs MC33's 0.105, Table 1) but
it is rotation-sensitive — *"sensitivity to rotation … the shapes in the ABC dataset are mostly aligned with
the coordinate axes"* — which is disqualifying for an editor where rotating a brush must not change the
result.

**Subgrid Marching Tetrahedra (`10.48550_arxiv.2606.00454`) — the most engine-relevant new result.**
Replaces per-vertex signs with **edge coordinates** (integer intersection counts per edge), generalizing
normal coordinates from 3-manifold topology.

| Claim | Figure | Where |
|---|---|---|
| Sample efficiency | same accuracy with **71M vs 125M** samples, **~7× fewer triangles** | p.10, Fig. 20 |
| Resolution equivalence | **180³** subgrid ≈ **500³** classic MT | p.11 |
| Termination | **O(n)** subdivisions for `Σe = n` | Thm C.1, p.17 |
| Subdivision rarity | requires **≥24 edge intersections** | p.7 |
| QEF regularizer | `λ = 0.1` for all examples | p.10 |
| Benchmark | 3,200 DORA models, *"nearly identical compute time"* | p.11 |

**Real theorems, with scope that matters:** manifold connectivity in the even-sum case, edge-manifold with
boundary-only non-manifold vertices in the odd-sum case (Thm A.1, p.15); **intersection-free proven for the
primal algorithm only** (Thm D.1, p.17) — the dual path, which is the one that preserves sharp features, is
labelled *"Requires normals, and lacks a no intersection guarantee."* Conformity across tets is by
construction. Global topology is explicitly **not** guaranteed (p.4).

The engine value isn't "subgrid detail" — it's that **dropping grid resolution 2.8× for equal error is a
~22× voxel-memory reduction and ~7× fewer triangles**, while the primal path is provably manifold and
intersection-free. Costs: you need all-roots-along-an-edge finding (analytic or interval), the
intersection-free proof doesn't cover the sharp-feature path, and the reference implementation is
single-threaded with no stencil table.

---

## 4. Dual methods, sharp features, adaptive LOD

### Surface Nets — `10.1007_bfb0056277`

Gibson 1998, MICCAI. Binary segmented volume in (not SDF, not Hermite). One node per surface cube, linked
to ≤6 neighbours, relaxed to minimize **sum of squared link lengths**, each node clamped inside its
original cube. 12 candidate triangles per node, **6 emitted** to avoid duplicates.

The paper is nearly number-free — no runtime, no triangle count, no memory figure anywhere. The one bound:
*"all surface elements have been constrained to lie **within 1 voxel** of the original binary
segmentation."* Relaxation counts shown: 10–100 iterations.

**Manifoldness is never claimed, never discussed, and the word does not appear.** Ambiguity is deliberately
left unresolved: *"the surface is pinched in at the net node, but neither a separating nor a bridging
surface is created"* — i.e. non-manifold vertices by design.

**Engine verdict:** cheapest thing in the family and the natural default. A Jacobi sweep with a box clamp,
trivially SIMD-able and chunkable. Two costs: (1) it is **iterative and history-dependent**, so a locally
re-meshed chunk won't match neighbours unless you run a fixed iteration count and relax a one-cube ghost
halo; (2) the pinch vertices will break edge-collapse simplification, half-edge structures, and most
collider bakers — budget a vertex-split pass.

### Dual Contouring — `10.1145_566570.566586`

Ju, Losasso, Schaefer & Warren, SIGGRAPH 2002. Its origin story is a destructible-terrain student game
capped at 64³. Shipped numbers (GeForce 3, error tolerance 0.01):

| Model | Quads | Time | Space |
|---|---:|---:|---:|
| part 64³ | 2,578 | **44 ms** | 1.3 MB |
| Chinese cube 128³ | 1,646 | 636 ms | 32 MB |
| temple 256³ | 39,201 | 3,586 ms | 156 MB |
| david 512³ | 143,533 | 2,948 ms | 91 MB |

**"The CSG operations (spheres of radius 6) in figure 1 took approximately 30 milliseconds to compute."**

Four things the folklore gets wrong: (a) **no crack patching, no restricted octree** — the minimal-edge
rule (*"only those edges of leaf cubes that do not properly contain an edge of a neighboring leaf should
generate a polygon"*) makes transition triangles emergent; (b) **QEF representation is worth 2× your
polygon count** — `bᵀb` reaches ~10⁶ on a 256³ grid so floats give error ~1; the QR/Givens form is also 10
floats and yields **36K polygons vs 78K** on the temple at equal tolerance; (c) SVD truncation threshold
**0.1**; (d) multi-material is free, with a 4⁸ quasi-manifold table.

Acknowledged in the paper itself: *"there exist sign configurations for which the dual contour is
non-manifold."* The topology-safety test handles this by **refusing to simplify**.

### Cubical Marching Squares — `10.1111_j.1467-8659.2005.00879.x`

Ho et al. 2005. Unfold each cube into 6 faces, run 2D marching squares per face, fold back, trace segments
into closed loops, fan-triangulate each around its sharp feature (SVD-solved) or centroid. Face ambiguity
resolved by **testing whether the two candidate sharp features would overlap** — *"Since the input data
describes a volume, it should not intersect with itself"* (p.5). Inter-cell dependency eliminated by
sampling a feature on the shared face rather than EMC's edge flip.

Accuracy vs DC and EMC (Table 2, p.7, averaged over random tetrahedra unions) — CMS wins nearly every MC
case, and on **case 13 (the tunnel case) it is 0.00100 vs DC 0.85461 and EMC 0.92935**, ~850× better.
Timing (Table 3–4, p.8, Pentium IV 3.2 GHz): CSG model at 3 LODs **1,688 / 4,880 / 14,568 triangles in
16.23 / 32.14 / 66.08 ms**; ~100k–360k triangles/second on 2005 single-core hardware.

**Crack-freedom is argued, and the argument is sound** (p.6): *"a crack happens where there exists an edge
owned only by a single component. This can only happen on the transition faces. In our algorithm, all edges
on the transition faces are generated from segments and every segment is exactly shared by two components
from two neighboring cells. Hence, the resulting mesh is guaranteed crack free."* Manifoldness is never
claimed. Self-intersection-freedom of the *input* is used as an assumption, which is not the same as
guaranteeing it of the output.

**Engine verdict:** the best structural fit for an editable-voxel editor that needs LOD. Cracks between
chunks at different octree depths are impossible by construction rather than by skirts. Costs: Hermite data
(≈3× voxel payload, CSG brushes must produce analytic normals), fan triangulation makes slivers around the
centre (bad for normal maps, worse for lightmap UVs), and component tracing is branchy — *"the resulting
speed is only comparable to our CPU implementation"* for their own GPU port.

### Neural Dual Contouring — `10.48550_arxiv.2202.01999`

NDC keeps classical DC's output structure and replaces the functions with 6-layer CNNs (~1 MB weights,
receptive field 7³). **UNDC drops signs entirely** and predicts the edge-crossing flag directly.

At 64³ (Table 2, p.7): NDC **10,969 triangles in 27 ms**, same budget as MC33's 10,954 — versus NMC's
85,544 in 1,148 ms. UNDC's chamfer is **0.930 vs NMC's 4.365** (×10⁻⁵), a 4.7× accuracy win driven almost
entirely by structures thinner than one voxel. On 4,096-point clouds with no normals (Table 6, p.10) UNDC
beats Poisson, Ball-Pivoting, SIREN and ConvONet on chamfer while emitting 11,261 triangles vs SIREN's
194,543.

Admitted: *"it can produce non-manifold meshes"*; *"the output of NDC is not completely invariant to
orientation … It cannot be easily avoided since it is due to the continuity of neural networks."*

**Engine verdict:** rotation-dependence is disqualifying for an editor. But **steal the UNDC idea** — store
a per-edge crossing flag rather than deriving faces from vertex signs — and drive it analytically from your
CSG evaluator. That gets you thin-sheet capability (a wall carved down to nothing) with no inference cost
and no rotation artifacts.

---

## 5. Terrain, voxel and runtime LOD

### The lineage, as the papers actually state it

**Lindstrom et al. 1996 (`10.1145_237170.237217`) is the only terrain paper whose stated requirements match
a destructible-terrain game.** Design criterion (ii): *"Dynamic changes to the geometry of the mesh should
not significantly impact the performance … recomputation … should be virtually instantaneous."* Criterion
(iii): *"local changes to the geometry should remain local."* And the motivating use case is named: *"The
importance of (ii) is relevant in many applications, such as games and military applications where
**explosions dynamically deform the terrain**"* (p.3). No edit cost is measured — the claim is structural.

Numbers: *"the number of rendered polygons per frame can be reduced by two orders of magnitude while
maintaining image quality such that less than 5% of the resulting pixels differ"*; sustained 20 fps over a
120-second, 3,230-frame flythrough at τ = 2 pixels (p.12–13). Crack prevention is a **vertex dependency
network** — a base vertex may be removed only if all vertices in its four subtrees have been. And the honest
admission (p.12): *"cross block dependencies are ignored, leaving occasional small gaps … such gaps have
been filled by vertical polygons."* **Skirts, in the citable original.**

**Hoppe 1998 (`10.1109_visual.1998.745282`) is where geomorphing is done right.** *"To our knowledge this
is the first runtime scheme for temporally smooth, view-dependent LOD control on arbitrary meshes."* The
transferable rule: **change connectivity instantly, defer only the geometry interpolation, and gate morph
initiation on visibility** — an offscreen region *"may have unbounded screen-space error"* and could pop
arbitrarily on entering the viewport. Also the exact-error argument: measuring residual only at grid points
understates error for irregular meshes; the fix is L∞ over the union partition, *"not just an upper-bound,
it is exact."* And the licence you need: **constraining block borders costs only 0.8% extra faces.**

**P-BDAM (`10.1109_visual.2003.1250366`) is the closest Nanite ancestor in the corpus.** Everything Nanite
does at cluster level, P-BDAM does at patch level five years earlier: batch a few hundred triangles as the
LOD atom, precompute a cache-coherent strip, keep it immutable in GPU memory, reduce the per-frame CPU job
to selecting a cut. Three things transfer verbatim:

1. **The simple edge error property** — the cleanest crack-free rule in the literature. Each triangle
   *"represents a small mesh patch with error `e_k` inside and error `e_{k−1}` … along the two shortest
   edges. In this way, each mesh composed by a collection of small patches arranged as a correct bintree
   triangulation still generates a globally correct triangulation."* Any valid cut is automatically
   watertight. No stitching, no skirts, no dependency graph. This is Nanite's locked-group-boundary
   invariant, stated a decade earlier and implementable in an afternoon.
2. **Camera-relative double→float at patch corners + barycentric interior vertices** — because the weight
   for the opposite vertex is null along an edge, *"neighboring patches remain unconditionally connected."*
   The correct answer to large-world float drift in Bevy, at 9 subtractions per patch.
3. **mmap + `madvise` speculative prefetch with half-second camera extrapolation** — 98 MB resident against
   5.7 GB of data, <2%.

On popping they refuse to blend: *"it is possible to use very small pixel thresholds, **virtually
eliminating popping artifacts, without the need to resort to costly geomorphing features**."* For
destructible terrain P-BDAM offers nothing — its economy rests on batches being immutable and pre-stripped.

**GigaVoxels (`10.1145_1507149.1507152`) is the best fit for true volumetric destruction** among the terrain
papers. Ray-guided demand-driven residency: the shader emits which nodes it wanted and at what LOD, the GPU
compacts that feedback, the CPU runs an LRU. *"Frustum containment, visibility and LOD selection are
naturally handled in one unified way and all workload is taken off the CPU."* Bricks can be **produced on
the GPU** — *"this on-the-fly noise creation proves actually to be more efficient than a CPU evaluation and
transfer"* (§7). No connectivity to repair, no error hierarchy to re-saturate, no seam to weld (quadrilinear
mip filtering handles transitions). Their own open problem: *"Currently, **animation is a big problem for
volume data**."*

**Sparse Voxel DAGs (`10.1145_2461912.2462024`) are the worst fit for destruction, and the paper says why.**
Compression is 28× even on HAIRBALL (*"no obvious regularities"*) and 576× on a normal scene. But sharing is
global — one node has many parents, so a single-voxel write requires un-sharing the whole path and re-running
the level-wise merge. Reduction cost sets the floor: **4.5 s for CRYSPONZA at 8K³**. Authors: *"Our focus has
not been on dynamic scenes which would require real-time compression."* Use it for static
shadow/AO/occupancy, keep hot chunks as plain SVOs or GigaVoxels bricks.

**Tanner's clipmap (`10.1145_280814.280855`), §9, is the most prescient paragraph in the corpus:**

> "It seems possible to develop a system that **stages geometric level-of-detail information for large
> databases similarly to the way clipmaps stage image data**. If an adaptive rendering algorithm were
> defined to create continuous tessellations from partially specified geometric levels of detail, then the
> same look ahead cache notions could be used to stage geometry."

That is virtualized geometry, described in 1998. Four ideas still apply: the **display-bounds argument**
(resident set bounded by screen resolution, not world size), **toroidal addressing**, **graceful degradation
as a first-class feature** (LOD faded over tens of frames, priority-sorted read queue, coarse levels first),
and the **InvalidBorder** (a ring the sampler provably cannot read, so you DMA into it without sync).

---

## 6. The real-time volumetric line — the big new section

**This is the closest thing in the literature to a spec for a destructible-terrain backing store.** v1 had
none of it.

### Curless & Levoy 1996 — `10.1145_237170.237269`

The representation and the update rule everything else inherits. Voxel stores `(D, W)`; observation
contributes `(d, w)`; merge is `D ← (WD + wd)/(W + w)`, `W ← W + w`. **Associative, commutative,
constant-work** — *"incremental and order independent updating … allow for straightforward
parallelization"* — and provably the least-squares surface (Appendix A, p.8). For an editor this is exactly
right: a dig/explode/deposit brush is just another weighted contribution, appliable in any order or in
parallel.

Also established: **truncation** (weights fall off *behind* the surface at half the max uncertainty
interval), which is what makes the field sparse and therefore compressible; and the **three-state
classification** — `empty` / `unseen` / `varying` — which for an editor is a principled "authored void vs
never-touched" distinction that gives free cap-surface generation at the boundary.

Costs: Dragon 61 scans → 1.7M triangles in **56 min**; with hole filling **257 min**; RLE savings 10:1–20:1;
accuracy 0.1 mm RMS, *"roughly the same as the accuracy of the scanning technology."* RLE is a streaming
codec, not an edit structure — a localized edit re-encodes a whole run.

### KinectFusion 2011 — `10.1109_ismar.2011.6092378`

Changed nothing about the math, everything about the layout: a flat dense array with *"a fixed bijective
mapping between voxel/memory elements and the continuous TSDF."* Strictly worse by every 1996 criterion,
and precisely why it runs in real time:

> **"over 65 gigavoxels/second (≈2ms per full volume update for a 512³ voxel reconstruction) can be
> updated"** (§3.3)

*"the simplicity of the kernel means operation time is memory, not computation, bound."* 16 bits per
component, *"as few as 6 bits are required for the SDF value."* Ray-skipping by μ gives *"≈6× speedup."*

**The 2 ms figure is the single most important number here.** A destructible system on a dense 512³ brick
does not need incremental edit tracking at all — you can re-evaluate every voxel against a list of active
brushes every frame and still have ~14 ms of a 16 ms budget left. The memory story is where it dies: 512³ at
4 bytes/voxel is 512 MB by arithmetic (the paper never prints MB), and *"volumes of ≤ 7m³"* is the stated
ceiling. Their own named fix: *"it would be possible to exploit sparsity in the TSDF using an adaptive grid
representation … there is only a relatively thin crust of non truncated values near the surface."*

Also worth noting §5: *"physics can be simulated in real-time on acquired models directly from the TSDF
volumetric representation"* with thousands of particles — **collision against the SDF, no collider mesh**,
which is exactly the trick that avoids rebuilding physics meshes on every edit.

### Voxel hashing 2013 — `10.1145_2508363.2508374`

Fixes exactly that, and explicitly refuses the octree: *"In an octree, the resolution in each dimension
increases by a factor of two at each subdivision level. This results in the need for a deep tree structure
… which conversely impacts performance, in particular on GPUs where tree traversal leads to thread
divergence."* Instead: flat hash from integer world coords → `8³` bricks in a preallocated GPU heap.

Hash `H(x,y,z) = (x·73856093 ⊕ y·19349669 ⊕ z·83492791) mod n`. 8 bytes/voxel, 12-byte hash entries,
bucket size 2.

**The measured frame budget** (i7 3.4 GHz + GTX Titan, §9.1):

> "Average timings among all test scenes is **21.8ms (~46fps)** with **8.0ms (37%)** for ICP pose estimation
> (15 iterations), **4.6ms (21%)** for surface integration, **4.8ms (22%)** for surface extraction and
> shading, and **4.4ms (20%)** for streaming and input data processing."

**The memory result:**

> "On average **less than 300MB** memory is allocated for surface data (**less than 600MB with color**).
> This compares favorably to a regular grid that would require **well over 5GB** … at the same voxel
> resolution (8mm) and spatial extent (8m in depth)."

Collision behaviour is empirically dead as a worry: *"all test scenes run with only **0.1% bucket
overflow** … the largest list length is three. In total ~700 linked list entries are allocated across all
scenes."* Scenes: a ~20 m corridor in under 5 minutes, a ~30 m passageway, a three-level bookshop — against
KinectFusion's 7 m³.

**Engine verdict:** maps onto a Bevy engine almost line for line — unbounded extent, O(1) integer-coordinate
access, dynamic alloc/dealloc during editing without touching neighbours, atomic per-bucket locking, an
explicit GPU free-list heap, GC of bricks that edits emptied, and 1 m³ host↔GPU streaming needing no
structural reorganization. Three caveats: (1) **deliberately flat, so no LOD** — distant terrain costs the
same per voxel as terrain underfoot; (2) the 4.8 ms extraction figure is **raycasting**, and a rasterizing
engine that wants triangles must add a mesher whose cost appears nowhere in this paper; (3) the 8³ brick has
no overlap ring, so every boundary trilinear fetch is a hash lookup — fine on GPU as measured, but a CPU
mesher in Rust will want the ring back, and they measured that at ~2× memory.

### What this means for your budget

| | Curless & Levoy '96 | KinectFusion '11 | Voxel hashing '13 |
|---|---|---|---|
| Whole pipeline | 47–56 min/model | not reported numerically | **21.8 ms (~46 fps)** |
| Fusion only | (not separable) | **≈2 ms per full 512³** | **4.6 ms** |
| Pose estimation | n/a | ICP, timing in figure only | **8.0 ms (37%)** |
| Isosurface | MC, offline | raycast | raycast, **4.8 ms** |
| Volume memory | 160M voxels, RLE 10–20:1 | 4 B/voxel, ≤7 m³ | 8 B/voxel, **<300 MB** vs **>5 GB** dense |

**Three things fall out.** First, **integration is cheap and tracking is expensive** — ICP is 8.0 ms while
writing the volume is 4.6 ms. A game engine has no ICP; it knows where its brushes are. The honest
edit-and-fuse budget is the 4.6 ms line, or KinectFusion's 2 ms for a fixed brick. Second, **sparsity bought
scale, not speed** — the dense sweep is competitive per frame and only loses on memory; hashing earns its
keep past ~7 m³. Third, **nobody in this line ever pays for polygonization in the loop.** All three raycast
or mesh offline. Any design that assumes it inherits these budgets *and* produces triangles per edit is
inheriting a number nobody measured — either budget the mesher separately, or raycast the SDF directly and
get the collision path for free.

---

## 7. Unstructured mesh generation

Additions from new primary sources.

**Chew 1989 (`10.21236_ada210101`)** — the first guaranteed-quality result. All output angles between
**30° and 120°**, all edge lengths between **h and 2h**; the optimal variant is **O(n)** because all changes
are local, *"within radius 4h of the newly added data point."*

**Shewchuk 1997 predicates (`10.1007_pl00009321`)** — the numerical substrate under every Delaunay, CDT and
mesh-boolean implementation you might write in Rust. This is the paper to read before implementing any of
§7; the adaptive filter is what makes exact predicates affordable.

**fTetWild venue version (`10.1145_3386569.3392385`)** and **CDT (`10.48550_arxiv.2309.09805`)** confirm
v1's numbers. Robustness on Thingi10k: **fTetWild 99.97%** vs TetGen 49.50% vs CGAL 79.00%.

The framing from v1 stands: **every timing in this family is offline.** Tet meshing is an asset-bake step.

---

## 8. Surface reconstruction

**Amenta & Bern 1999 (`10.1007_pl00009475`)** replaces v1's second-hand Crust characterization with the
theorems. **Theorem 2: for an r-sample with r ≤ 0.1, the good triangles form a polyhedron homeomorphic to
the surface.** Theorem 4 (r ≤ 0.06) bounds the crust inside a fattened surface of radius 5r·LFS. Zero
tunable parameters. Only one runtime remark in 24 pages and one dataset size (3,511 points) — it is a theory
paper.

Two things worth stealing: the **pole construction as a normal estimator** (comes with an error bound that
kNN-PCA does not; Hoppe's kNN approach fails on *"arbitrarily dense sets of samples … with almost collinear
nearest neighbor sets"*), and the **local-feature-size sampling criterion** — required sample spacing scales
with distance to the medial axis, dense at thin features and creases, sparse on flat walls. That is a
directly implementable retopo/decimation heuristic.

**FSSR (`10.1145_2601097.2601163`)** — per-sample **scale** as a first-class input, not density as a proxy.
Deliberately **not watertight**: *"reconstructs open meshes and leaves holes in regions where data is too
sparse"*, explicitly contrasted against Poisson which *"hallucinate[s] geometry in incomplete regions."*
For game assets the open-mesh behaviour is arguably a feature. The memory curve is brutal: 196M samples →
19.9 GB; 472M → **114 GB / 4 hours**. Chunk spatially.

Screened Poisson remains the default (14–20 s at depth 8–9), with the denoising pre-pass as the
highest-leverage 30 seconds in an import pipeline.

---

## 9. Simplification, remeshing, quad

**Hoppe 1999 (`10.1109_visual.1999.809869`)** — the wedge-based quadric for appearance attributes, the piece
v1 was missing between Garland-Heckbert '98 and the modern textured simplifiers.

**Dunyach et al. 2013 (`10.2312_conf_eg2013_short_029-032`)** — the only paper in this family claiming a
real-time budget, and the curvature-adaptive sizing field is the portable editor-side remesher.

**QEx (`10.1145_2508363.2508372`)** — robust quad extraction from integer-grid maps; the missing piece
between a cross field and an actual quad mesh.

**Directional field synthesis STAR (`10.1111_cgf.12864`)** — the survey that organizes the whole
cross-field/frame-field/PolyVector space.

v1's two structural findings stand: **98.9% of ShapeNet is non-manifold**, so any simplifier you ship must
eat triangle soup; and **the UV-preservation fight is probably not winnable** — the corpus's best answer
(STMW) abandons UV preservation and re-bakes texture through the decimation history.

---

## 10. Differentiable extraction — the comparison

Two independent measurement campaigns exist, on different datasets, losses and tooling. **They disagree by
~2× on FlexiCubes' triangle quality and ~3× on its self-intersection rate.** Both shown.

**Source A** = FlexiCubes paper, 79 watertight Myles shapes, depth+silhouette+SDF loss, 1000 iters.
**Source B** = TetWeave paper, 75 ThreeDScan shapes, mask+depth+normal loss, 5000+2000 iters, PyMeshLab.

| Metric | MC | DMTet | FlexiCubes | TetWeave | Src |
|---|---|---|---|---|---|
| Self-intersecting tris | 0.0% @64³ | 0.0% | **0.10%** @64³ | — | A §7.2 |
| Self-intersecting faces | — | 0.000% all res | 0.775/0.341/0.203% @32/64/128³ | **0.000% by construction** | B T5 |
| Aspect ratio >4 | 11.46% | 17.31% | **2.93%** | — | A T4 @64³ |
| Aspect ratio >4 | — | 12.8/11.6/12.0% | 7.3/6.1/5.4% | **2.25–2.51%** | B T5 |
| Slivers <10° | 11.82% | 17.83% | **2.04%** | — | A T4 |
| Slivers <10° | — | 13.0/11.9/12.4% | 6.4/5.2/4.6% | **1.6–1.9%** | B T5 |
| Extraction fwd/bwd @64³ | 2.28/0.43 ms | 2.33/1.38 ms | 8.93/7.32 ms | 162 ms @64K (98% Delaunay) | A T7 / B T3 |
| Extraction memory | 12/73 MB | 22/168 MB | **117/816 MB** | — | A T7 |
| Guarantees | int-free ✓, manifold ✗ | ✓✓✓ | manifold ✓, watertight ✓, **int-free ✗** | **✓✓✓ by construction** | A T1 / B T1 |

**Where they disagree.** FlexiCubes self-intersection: 0.10% (A, their own tooling, Myles) vs 0.341% at 64³
(B, PyMeshLab, ThreeDScan). B's trend decreases with resolution (0.775 → 0.341 → 0.203), consistent with A's
figure sitting at the optimistic end of that band. Triangle quality differs 2–3× in the same direction.
A supervises with a ground-truth SDF term; B is pure inverse rendering. **A is the best case; B is the
realistic case for photogrammetric input.**

**Triangle quality is bought with a regularizer, not with the extractor.** Both papers price it, from
opposite directions: A adds an equilateral-edge regularizer and FlexiCubes' AR>4 drops 2.93% → **0.59%**,
slivers 2.04% → **0.24%**, at the cost of normals 34.87 → 41.05 and CD 4.87 → 5.46. B *removes* its fairness
loss and TetWeave's AR>4 rises 2.251% → **17.091%**, slivers 1.616% → **15.611%**, with vertex count +57%.

**Bottom line:** DMTet is the fast, guaranteed-clean, sliver-ridden baseline. FlexiCubes trades the
intersection-free guarantee (0.1–0.8% of triangles) for 2–5× fewer slivers and better sharp features —
**the right default**. TetWeave restores every guarantee and halves slivers again, but is 20–70× slower,
emits 2–3× more triangles, and drops holes in thin structures — right for closed organic props where a
guaranteed-manifold intersection-free collider is worth it.

**SuGaR is a trap worth naming.** Table 3: the same 200K-vertex mesh rendered with a **conventional UV
texture** loses ~2.8–3.3 dB PSNR and nearly doubles LPIPS versus rendering it with bound Gaussians. The mesh
is a *scaffold* for a splat renderer, not a textured model; thin geometry (bicycle spokes) exists only in
the Gaussians and **not in the mesh at all**. Dropping 1M → 200K vertices costs only 0.27 dB, so decimation
is nearly free — but you inherit two Poisson shells, not one watertight object.

---

## 11. Generative meshing — the artist-topology claim, tested

**Verdict: mostly qualitative and user-study based, with one narrow measured exception that the follow-up
paper reversed.**

Across PolyGen → MeshGPT → MeshAnything → MeshAnything V2 → EdgeRunner, **not one paper reports a manifold
percentage, watertight percentage, quad ratio, singularity count, or edge-flow measure.** "Topology quality"
is one of three things: FID/KID on **rendered wireframe images**, face/vertex counts, or human votes.

| Model | Face cap | Compression | Training cost | Human eval |
|---|---:|---|---|---|
| PolyGen 2020 | 800 (sampling) | — | 4× V100, no wall-clock | none |
| MeshGPT 2024 | **768** | 6 tok/face (1.5×) | **≈576 A100-h** | 49 participants / 784 responses |
| MeshAnything | **800** | (inherits) | not reported | 41 users / 1,230 comparisons |
| MeshAnything V2 | **1,600** | AMT **0.492** (4× attention saving) | **≈3,072 A800-h** | 43 users / 30 meshes |
| EdgeRunner | **4,000 @512³** | 4–5 tok/face (~50%) | **≈13,400 GPU-h** | **8 test cases**, participant count never stated |
| TRELLIS (iso-surface) | n/a (FlexiCubes @256³) | n/a | 64× A100, 400K steps | **104 participants / 2,701 trials** |

**Strongest evidence for the claim** — MeshAnything §A.2, the only measured case of a generator emitting
*fewer* primitives than the artist, **F_Ratio 0.871 / V_Ratio 0.888**:

> "our method can produce results with fewer faces than the ground truth, demonstrating that MeshAnything is
> not overfitting … but instead **learns an efficient topology representation, occasionally surpassing the
> ground truth meshes**."

And EdgeRunner's one *structural* guarantee: *"The traversal ensures that **each face's orientation remains
consistent within each sub-mesh**. Consequently, the generated mesh can be accurately rendered using back
face culling, a feature not consistently achieved in prior methods."*

**Strongest evidence against.** MeshAnything concedes the metrics cannot see topology: *"the metrics in mesh
extraction can only indicate the quality of shape alignment, which **do not effectively reflect the
topological advantages** of our method"* — paired with §A.1 revealing that the topology number is FID on
rendered wireframes. EdgeRunner: *"**Automatically evaluating the aesthetic quality of mesh topology is
challenging, so we rely on a user study.** We selected 8 test cases…"*

And the decisive one — MeshAnything V2 measured the compactness claim at double the budget and it went the
wrong way:

> "both AMT and the variant have **vertex and face ratios greater than 1.0, meaning they use more faces on
> average relative to the ground truth**, unlike the results in [V1] … the model [may] **occasionally
> produce more complex topology for simple shapes**."

V2's limitations section, in full: *"the accuracy of MeshAnything V2 is still insufficient for industrial
applications."*

Two structural facts reinforce it. **MeshGPT's raw output is not a mesh** — *"this output initially forms a
'triangle soup' with duplicate vertices … we apply a simple post-processing operation to merge close
vertices (e.g., with MeshLab)"* — so connectivity is decided by a welding tolerance, not by the model.
And **none of them emit UVs, materials, or rigs.**

**The honest summary:** the line delivers a real, measured win on **face count** — MeshAnything at ~318
faces vs Marching Cubes at ~146,000 for the same object, a ~460× reduction. It delivers one real guarantee
on **face winding**. It delivers **nothing measured** on manifoldness, watertightness, quad flow or
singularity placement — the properties that decide whether an artist can subdivide, unwrap, skin or deform
the result. *"Artist-like"* in these papers means **as few triangles as an artist would use, judged by eye**.
It does not mean *structured the way an artist would structure it*, and nobody claims to have measured that.

TRELLIS is the strongest *input* — a shape in ~10 seconds at 94.5% image-fidelity preference (n=104,
uncurated) — but hands you a dense FlexiCubes iso-surface at 256³ with **baked-in lighting** (*"we leave
[PBR materials] for future exploration"*), vertex colours instead of a texture, and no material graph. In a
Bevy PBR pipeline baked highlights are a correctness bug: the asset gets lit twice.

---

## 12. Collision proxies and navmesh

**CoACD (`10.48550_arxiv.2205.02961`) is the one generative-adjacent result in this whole document that is
shippable unattended**, because its output is defined by a satisfied constraint rather than a preference
vote: concavity ≤ ε with intersection-free hulls, guaranteed by cutting the solid with 3D planes instead of
voxelizing.

| vs | Components | Concavity |
|---|---|---|
| HACD | ≈57.6 → **29.6** (V-HACD set); ≈33.5 → **7.3** (PartNet) | 0.118 → 0.084; 0.414 → 0.204 |
| V-HACD | ≈60.2 → **29.8**; ≈44.6 → **20.1** | — |
| greedy vs MCTS | 49.9 parts / 271.7 s → **34.5 parts / 229.8 s** | fewer parts *and* faster |

**The number to quote** (Table 3): SAC agents opening 49 drawers from 25 cabinets in SAPIEN —
**V-HACD 49% → CoACD 80% success**, because *"when using V-HACD's collision shapes, the robot arm easily
slips off the handles, since they **fill the holes**."* Collision-proxy quality is not cosmetic; it decides
whether interaction works at all.

Contract: *"we assume that the input is a **2-manifold solid mesh**."* Defaults m=20, t=500, d=4, k=0.3.
Runtimes 67–270 s single-threaded — offline bake.

**DEACCON / ASFV3D (`10.1609_aiide.v4i1.18693`, `10.1609_aiide.v5i1.12376`)** — grow-and-seed navmesh
decomposition. DEACCON: **100% coverage**, significantly shorter paths and fewer turns (p<.05) across 5
Urban Terror maps, replacing a manual task the paper prices at *"several days per environment."* The 2009
3D successor adds **gravity-aware seeding**, and the rationale is the transferable insight: without it,
*"a set of stairs … creates a navigation mesh that implies that agents can float up from the bottom of the
stairs and end up half way up the stair case."* Note the 2009 paper is validated on **n=1 test world** with
no timings — the 2008 predecessor is the better-validated one.

Both require convex positive-space input, which is a neat argument for pairing them with CoACD in one
pipeline.

---

## 13. What is still missing

**Genuinely absent, blocking:** Transvoxel (Lengyel 2010 — the dissertation is free at transvoxel.org);
Manifold Dual Contouring; Intersection-free Contouring on an Octree Grid; Extended Marching Cubes (Kobbelt
2001); geometry clipmaps; ball-pivoting; original Poisson 2006; TetGen; TetWild; isosurface stuffing;
appearance-preserving simplification (Cohen 1998); Botsch & Kobbelt remeshing; mixed-integer
quadrangulation.

**Still-unmeasured, in any paper:** re-mesh latency after a *blocky* voxel edit (the only edit-latency
figure anywhere is DC's ~30 ms CSG sphere); per-chunk memory accounting for a voxel world; incremental
navmesh rebuild timing; Nanite build times; anything Bevy/Rust/wgpu-specific.

**The most important unmeasured thing:** no paper in the real-time volumetric line reports a **Marching
Cubes / dual-contouring frame cost**. They all raycast. If your design produces triangles per edit, you are
budgeting from numbers nobody measured.

**Corpus health:** 342 documents remain readable-but-unsearchable, 101 Qdrant orphans, 362 stuck
conversions, ~14 HTML paywall stubs from today's downloads, and one wrong paper
(`10.48550_arXiv.1806.02158`, a physics paper filed as TetWild). See the curation prompt.
