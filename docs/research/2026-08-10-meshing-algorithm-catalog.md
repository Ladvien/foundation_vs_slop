# Meshing algorithms in home-still — catalog with engine tradeoffs

**Date:** 2026-08-10
**Question:** what meshing algorithms does the local corpus actually contain, and what does each one cost
you inside a Bevy game/editor?
**Method:** seven parallel sweeps over the 8,648-entry / 253,240-chunk corpus (~60 `distill_search`
queries), `catalog_read` on every hit to resolve real titles, `markdown_read` on specific pages to pull
numbers, then an adversarial verification pass over all 69 stems and every headline figure. Every number
below has been read out of a corpus document — nothing here is from memory.
**Companion docs:** `2026-08-09-grid-composition-corpus-check.md`, `2026-08-08-editor-model-design-guide.md`.

---

## 0. Verdict up front

**Read §9.0 first if you read nothing else.** The verification pass found that `distill_search` cannot see
**342 documents** in this corpus — they converted fine, `markdown_read` works, but they have zero Qdrant
chunks. One of them is **Dual Contouring of Hermite Data**. Another is **Adaptively Sampled Distance
Fields**. Semantic search reported both as absent; catalog enumeration found both. Any "the corpus doesn't
have X" conclusion reached by search alone in this or any earlier doc is unsound.

With that corrected: **the corpus is strong on isosurface theory, production LOD, and the differentiable-
meshing line; the remaining hole is LOD seam-stitching.** You have Marching Cubes in depth (original, MC33,
ambiguity, GPU, SIMD, survey), you have Dual Contouring *and* ASDF, you have Nanite and CBT terrain from
the horse's mouth. You do not have Transvoxel, Surface Nets, Manifold DC, Extended MC, or geometry
clipmaps — and Transvoxel is the one that actually blocks you.

Second: **almost nothing in this corpus is fast.** Of ~45 algorithms catalogued, five have published
per-frame budgets (Nanite, CBT terrain, ROAM, Progressive Buffers, DC's 30 ms CSG edit), and four of the
five are *LOD selection over already-built geometry*, not meshing. Every other mesh-*generation* algorithm
with a timing is 10²–10⁶ ms. Meshing is a bake step or a worker-thread step in almost every source here.

Third, and most useful for `foundation_vs_slop`: **the corpus quantifies the generated-asset topology
gap.** QUADify measures DMTet at 16.89% sliver triangles; TetWeave measures DMTet(128³) at 12.35%
self-intersecting faces; "Prompted Props" measures a Hunyuan 3D scene at 111.6× the triangle count of
the hand-authored equivalent, with UVs that break under decimation. That is the argument, with numbers.

### The six axes

| Axis | What it means here |
|---|---|
| **Speed class** | `per-frame` / `worker-thread` / `bake` / `offline` (minutes–hours) |
| **Editable** | can you change the field and re-mesh a local region cheaply? |
| **Sharp** | does it preserve creases and corners, or round them off? |
| **Sound** | manifold + watertight + intersection-free — what physics and further processing need |
| **Seams** | do adjacent chunks/LOD levels stitch without cracks? |
| **Budget** | triangle/vertex cost per unit of surface |

---

## 1. The one comparison the corpus actually ran

`10.33842_2313-125x-2023-25-11-21` — Ausheva, Chornyi, Kardashov, Onysko & Tarnavski (2023),
*"Procedural generation of voxel landscapes based on isosurfaces using multithreading"*. A UE5 C++ plugin,
identical scene for all four meshers: 64 chunks, 32-voxel chunks, 3D Perlin, seed 1337, 1 octave, freq 0.03.
This is the only apples-to-apples benchmark of voxel meshers in the corpus.

| Mesher | Triangles | Vertices | Max main-thread stall | With multithreading |
|---|---:|---:|---:|---:|
| Cubic, no face culling | 915,588 | 610,392 | 10,869 ms | 8,843 ms |
| Cubic + face culling | 18,492 | 12,328 | 1,842 ms | 715 ms |
| **Greedy meshing** | **6,690** | **4,460** | 2,357 ms | **461 ms** |
| Surface Nets | 18,492 | 12,328 | 2,050 ms | 811 ms |
| Marching Cubes | 17,808 | 17,808 | 1,824 ms | 1,080 ms |

Read it carefully — it inverts the usual intuition:

- **Greedy is the slowest single-threaded and the fastest multithreaded** (2,357 → 461 ms, 5.1× from
  threading; MC only gets 1.7×). Greedy's merge sweep is embarrassingly parallel per chunk; MC's is not,
  in their implementation.
- **Marching Cubes has the worst vertex buffer of the four** — 17,808 verts for 17,808 tris, i.e. zero
  vertex sharing. That is an implementation artifact, but it is the one you get if you write MC naively
  from the paper.
- Surface Nets matching face-culled cubes exactly (18,492 / 12,328) is structural, not a typo: naive
  surface nets emits one quad per sign-changing grid edge, which is exactly the exposed-face count. The
  identical *vertex* count says their implementation didn't weld the shared cell vertices — welded surface
  nets should be well under 12,328.
- Hidden-face removal alone is worth **~49.5×** over naive cubes. Do that before anything clever.

**Caveat the paper itself does not address:** no LOD, no chunk seams, and — the number you actually need —
**no re-mesh latency after an edit.** Nothing in the corpus measures ms-to-remesh-a-chunk-after-a-block-place.
That gap is called out again in §9.

---

## 2. Family A — isosurface extraction on regular grids

The best-covered family in the corpus by a wide margin.

| Algorithm | Corpus stem | Sharp | Sound | Cost vs plain MC |
|---|---|---|---|---|
| Marching Cubes (1987) | `10.1145_37401.37422` | no | **cracks** | baseline |
| Asymptotic Decider (1991) | `10.1109_visual.1991.175782` | no | face-consistent | ~free |
| Montani modified LUT (1994) | `10.1007_bf01900830` | no | consistent | "no computational overhead", 5 tris/cell max |
| Discretized MC (1994) | `10.1109_visual.1994.346308` | no | ½-cell error | **7–12× fewer facets**, 85% of time in merge |
| Adaptive Skeleton Climbing (1998) | `10.1111_1467-8659.00261` | no | **gap-free by construction** | 4–25× fewer tris, comparable time |
| Marching Tetrahedra / Wrapper | `10.1109_2945.485620` | no | no ambiguity by construction | **2–3× more vertices** |
| MC33 w/ topological guarantee (2003) | `10.1080_10867651.2003.10487582` | no | **manifold, guaranteed** | 730 subcases in the LUT |
| SIMD/SWAR MC (2003) | `10.1016_j.cag.2003.12.008` | no | = MC | ~2× vs optimized C, ~3.75× vs naive |
| Extended LUT / SnapMC (2008) | `10.1111_j.1467-8659.2008.01209.x` | no | **topologically wrong** (see below) | 25–40% fewer tris at ~2× time |
| Edge transformations (2008) | `10.1109_tvcg.2008.60` | no | zero degenerate tris | ≤2× MC time |
| GPU MC + accel structures (2012) | `10.1007_s13173-012-0097-z` | no | = MC | **~18× vs CPU** |
| Extended MC33 triangulation (2019) | `10.1186_s13173-019-0086-6` | no | fixes MC33's non-manifold edges | grid preprocessing pass |
| Subgrid Marching Tetrahedra (2026) | `sig2026_Subgrid_Marching_Tetrahedra` | no | **manifold + intersection-free, proved** | ~7× *fewer* tris at equal error |

### The ones that matter for you

**Marching Cubes, original.** `10.1145_37401.37422`. Worth reading for one engineering detail the
reimplementations drop: slice/line/pixel coherence means only 3 new edges (6, 7, 12) need interpolation
per interior cube — the other 9 come from neighbours already computed. *"Using the coherence speeds up
the algorithm by a factor of three"* (p.4). The paper makes **no** topological claim; the holes were
reported a year later by Dürst.

**MC33 with topological guarantees.** `10.1080_10867651.2003.10487582` (Lewiner, Lopes, Vieira, Tavares,
*J. Graphics Tools* 8(2), 2003). This is the implementable one — three tables plus explicit face and
interior tests, *"730 subcases of the enhanced look-up table"*, producing *"a manifold surface, with no
crack, with the topology of the trilinear interpolation"* (p.4). Ambiguity tests are needed only for
cases **3, 4, 6, 7, 10, 12, 13**. The face test reduces to `sign(face_label · F(A) · (F(A)F(C) − F(B)F(D)))`
(p.13). **If you implement one MC variant in Rust, implement this one** — the 256-entry table alone
produces cracks (Fig. 3, p.6).

**How often does ambiguity actually bite?** The survey `10.1016_j.cag.2006.07.021` (Newman & Yi, 2006):
*"one small study suggested that typically about 3% (and at worst 5.6%) of active cells exhibit face
ambiguity"* (p.17). Small, but it's a hole in your mesh — and holes fail collision.

**Extended LUT / SnapMC is a trap.** `10.1111_j.1467-8659.2008.01209.x` (Raman & Wenger, CGF 2008)
reports genuinely attractive numbers — 25–40% triangle reduction at γ=0.3, CPU time essentially unchanged
for the extended table (aneurism 0.71→0.73 s). But `10.1186_s13173-019-0086-6` (Custodio, Pesco & Silva,
2019) shows its base table *"covers only 23 of the 33 possible behavior of the trilinear interpolant"*,
and measures the damage: on a torus case, correct Betti numbers are (1,2,1), Raman & Wenger gives b₀=4
with **b₁=0** — genus destroyed. Take the reduction from Discretized MC or ASC instead.

**Discretized Marching Cubes is the academic ancestor of greedy meshing.** `10.1109_visual.1994.346308`
(Montani, Scateni, Scopigno, IEEE Vis '94). Replace edge interpolation with midpoints → only 13 vertex
positions and 13 plane incidences exist → most adjacent facets are exactly coplanar → merge them.
Sphere 100³: **37,784 → 5,501 facets**. Buckyball 128³: **204,408 → 17,039** (12×). Head: 428,181 → 57,413,
output as 13,005 tris / 34,856 quads / 9,552 n-gons. Runs in **integer arithmetic** except normals.
Cost: max error **½ cell size**, and **85% of runtime is the merge pass**. The direct analogue for a
blocky/low-poly voxel engine.

**Subgrid Marching Tetrahedra is the interesting new one.** `sig2026_Subgrid_Marching_Tetrahedra` (Baktash,
Gillespie & Crane, TOG 45(4) Art. 57, 2026, DOI 10.1145/3811358). Replaces per-vertex signs with *edge
coordinates* — integer counts of how many times each grid edge pierces the surface. Consequence:
*"puts no lower bound on the size of features that can be resolved on a fixed grid"*, sidestepping Nyquist.
Output is manifold, closed, orientable **and intersection-free** (Theorem A.1). Measured:
*"the same accuracy as classic marching with far fewer samples (71M vs 125M)… with a dramatically smaller
output mesh, ~7× fewer triangles"*; subgrid at **180³** matches classic MT at **500³**. It also accepts
**polygon soup with no consistent orientation** and returns a manifold intersection-free mesh — that is an
asset-repair primitive, not just a mesher. Limitation: single-threaded reference implementation, no
precomputed stencil tables, and *"we do not carefully treat the question of how to split quads into
triangles"*.

**Adaptive Skeleton Climbing** `10.1111_1467-8659.00261` (Poston, Wong & Heng, CGF) deserves a look for
the seam property alone: *"we do not need a crack-patching step because we build compatibility… into the
faces where cells meet before generating triangles"*. knot256: MC 225,736 tris / 94.62 s vs ASC N=8
**8,829 tris / 87.78 s** — 25.6× fewer triangles *and* faster. But it is not uniformly good: Head at N=4
is 4.3× fewer tris but **1.6× slower**, and at N=1 it is 5.5× slower than MC. Optimal block size was
N=4 in three of six tests. Only a **16-entry 2D table** is needed — no 3D cube configuration table at all.

---

## 3. Family B — dual methods, sharp features, adaptive LOD

### 3.1 Dual Contouring of Hermite Data — you have it, and it is better than its reputation

`10.1145_566570.566586` — Ju, Losasso, Schaefer & Warren (Rice), SIGGRAPH 2002. **8 pages of clean
markdown, invisible to `distill_search` (zero chunks). Read it directly.**

Its origin story is your project: *"In the spring of 2001, 17 students in an advanced computer graphics
class set out… One of the primary goals for the game was to incorporate technology that allowed real-time
modification of the game geometry."* Their post-mortem lists exactly three problems, which are the three
you will hit:

1. *"The resulting environment lacked the sharp edges found in most polyhedral models."*
2. *"The polyhedral meshes produced by contouring often contained large flat regions tiled by numerous
   small polygons… trivially inflated the number of polygons and often overwhelmed the graphics card."*
3. *"Due to the use of a uniform grid, we were restricted to relatively small grid sizes… the final game
   could only process environments defined using grids of size 64³ due to the requirement that the game
   run in real-time."*

DC is the fix for all three. **Shipped numbers** (consumer PC, GeForce 3, simplified to error tolerance
0.01, unit grid spacing) — time = octree simplification with topological safety + polygon generation:

| Model | Quads | Time | Space |
|---|---:|---:|---:|
| part 64³ | 2,578 | **44 ms** | 1.3 MB |
| Chinese cube 128³ | 1,646 | 636 ms | 32 MB |
| temple 256³ | 39,201 | 3,586 ms | 156 MB |
| david 512³ | 143,533 | 2,948 ms | 91 MB |

And the number that matters most for a destructible editor: **"The CSG operations (spheres of radius 6)
in figure 1 took approximately 30 milliseconds to compute."** That is the only interactive edit-latency
figure in the entire corpus.

**Four things the folklore gets wrong, per the paper itself:**

- **It does not need crack patching, and it does not need a restricted octree.** *"This new method imposes
  no constraints on the octree (such as being a restricted octree) and requires no 'crack patching'."*
  The rule is the **minimal edge** rule: *"only those edges of leaf cubes that do not properly contain an
  edge of a neighboring leaf should generate a polygon."* Where a coarse cube is face-adjacent to four fine
  cubes, minimal edges in the middle of the shared face are contained by only *three* cubes and generate
  **transition triangles** between coarse and fine quads — the seam handling is emergent, not bolted on.
  It is *Perry & Frisken's* rule that yields cracks and needs extra subdivision, not this one.
- **QEF numerical representation is worth 2× your polygon count.** The standard `AᵀA / Aᵀb / bᵀb` packing
  (10 floats) is unstable: on a 256³ grid `bᵀb` reaches ~10⁶, floats carry ~6 digits, so `E[x]` evaluated
  on a flat region *"has an error on the order of 1."* Their QR/Givens-rotation representation is also
  10 floats and fixes it. Measured on the temple at error tolerance 0.014: **78K polygons (standard) vs
  36K polygons (QR)**. Cost: merging two QEFs is ~150 arithmetic ops instead of adding 10 floats.
- **SVD truncation threshold is 0.1.** Nearly-coplanar normals make `Â` near-singular and the minimizer
  flies outside the cube; they truncate singular values below **0.1** and minimize distance to the centroid
  of the intersection points.
- **Multi-material is free.** Replace signs with material indices; a quad spans any *index* change. Three or
  more materials in a cube put the vertex at their intersection point, which *"allows the outlines of
  letters and characters embossed on a surface to be reproduced very accurately"* — cube-based methods
  can't, since their vertices are pinned to grid edges. Topology safety generalizes via a **quasi-manifold**
  test (each material's boundary is separately manifold), precomputed into a 4⁸ table (2⁸ for two-material).

**The honest limitation is in the paper, not just in its critics:** *"there exist sign configurations for
which the dual contour is non-manifold. (These configurations correspond to the 'ambiguous' sign
configurations in standard cube-based methods.)"* Their topology-safety test handles this by **refusing to
simplify** — three checks: coarse contour manifold, each fine cube manifold, fine contour topologically
equivalent to coarse on every sub-face. Fail any → stop collapsing. The manifold test itself is Gerstner &
Pajarola's: repeatedly collapse cube edges whose corners share a sign; manifold iff the result is a single
edge.

Traversal is `faceProc` / `edgeProc` mutual recursion (quadtree; `cellProc`/`faceProc`/`edgeProc` for
octrees) — enumerate sets of cubes sharing a minimal edge rather than enumerating edges and hunting for
neighbours, which *"entails either walking up and down the octree or explicitly maintaining links."*

Its companion, **Adaptively Sampled Distance Fields** (`10.1145_344779.344899`, Frisken, Perry, Rockwood &
Jones, SIGGRAPH 2000, 707 citations) is also in the corpus and also unsearchable. That is the octree
storage layer DC assumes.

### 3.2 What the rest of the corpus says about DC from the outside

Worth reading alongside, because the criticism is real even if the crack claim isn't.

| Source | Claim about DC |
|---|---|
| `TetWeave-…` p.3 | *"leverages the dual of an octree grid and optimizes vertices inside cubes based on normals to recover sharp features better, but **produces non-manifold, self-intersecting meshes**"* |
| `10.1007_s00371-021-02139-w` Table 8 | non-manifold vertices: **DC 5.61%** vs DMC 0.84% (skull); DC 2.15% vs 0.26% (skeleton) — *"one order of magnitude higher"* |
| same, Table 6 | iso-value deviation **DC 39.09 vs DMC 0.00** — *"DC uses quadratic error functions… vertices are in general not positioned on the iso-surface"* |
| `10.1016_j.cad.2016.07.009` p.3 | *"often results in an undesired flat angle degrading the mesh quality"*; needs pillowing |
| `s2015-advances-*` p.43 | (Media Molecule) *"Hey this is easy on GPU. Oh but it's kind of hard to keep sharp edges sharp and smooth things smooth and it doesn't really align to features for edge flow either"* |
| `sig2026_Subgrid_Marching_Tetrahedra` §1 | adaptive methods *"lose the simplicity and efficiency of regular marching, and may still fail to identify surface-edge intersections"* |

**The Dreams post-mortem is the most valuable thing here.** `s2015-advances-compressed-34mb-…` — Alex Evans,
*"Learning from failure: A Survey of Promising, Unconventional and Mostly Abandoned Renderers for Dreams PS4"*.
Read pages 26 and 46–52 before you commit to a DC-based editor. The arc, verbatim:

- p.46 — *"the DC mesh is still quite dense… occasionally what should be a straight edge ends up wobbly
  because it cant decide if this should be smooth or straight. **VERY tricky to tune in the general case for UGC.**"*
- p.47 — *"Oh no, there are self intersections! This makes the lighting look glitched - fix em"* → apply
  Ju & Udeshi intersection-free contouring
- p.48 — *"Oh no, now it's not necessarily manifold, fix that"* → apply Manifold Dual Contouring
- p.49 — *"Oh no, it's self intersecting again. Maybe marching cubes wasn't so bad after all... and
  **LOD is still hard (many completely impractical papers)**. ARGH! **Manifold? Non-Self-Intersecting? Pick one :(**"*

They abandoned the whole branch. For a UGC/editor context — which is yours — that is the single most
relevant data point in the corpus. Note the tension with §3.1: Ju et al. report DC as crack-free and
constraint-free on the octree; Media Molecule report it as unshippable for UGC thirteen years later. The
difference is *scale and generality* — Ju et al.'s hardest case is a 512³ David at 3 s, Evans is meshing
arbitrary user-sculpted content in a shipping frame budget with LOD. Both are true.

Two implementation details from the same deck worth keeping even if you don't use DC:

- p.26 — octree split granularity: *"we actually split by 4×4×4 in one go, which fits GCN's 64 wide
  wavefronts"*; and the tolerance rule — *"if you err on the 'too little split' side, you get gaps in the
  model. most of the rendering backends we were trying required at least 1 to 1.5 voxels of valid data on
  each side of the mesh"*. **Also: "the splits must be completely seamless."**
- pp.50–52 — mesher output ordering: *"non deterministic vertex/index order on output from a mesher, cache
  thrashing hell"*, fixed with GCN `DS_ORDERED_COUNT` append → *"hilbert ordered dual contouring! so much
  better on your (vertex) caches."* Directly portable idea for a Bevy chunk mesher: **sort your output,
  don't emit in discovery order.**

### The dual methods you *do* have

**Nielson's Dual Marching Cubes.** `10.1109_visual.2004.28` (IEEE Vis 2004). Different from
Schaefer & Warren's DMC — this one takes the dual of the MC *patch* surface: one vertex per voxel patch,
one **quad** per MC vertex. Quad-only by construction (every MC vertex lies on a lattice edge shared by
exactly 4 voxels). 256 cases → **23 configurations** under rotation. Manifold where DC, SurfaceNets and
cuberille are not: *"The separating surfaces described here have this manifold property. This comes at the
expense of dealing with more than one vertex per voxel"* — and the multi-vertex configurations
*"typically comprise about 1.3% of all configurations"*. Honest about the gap: *"How to define and compute
the MC-Dual vertices is a wide open research topic"* — it only uses patch centroids, no Hermite data, so
**no sharp features**.

**Parallel/GPU Dual Marching Cubes.** `10.1007_s00371-021-02139-w` (Grosso & Zint, *Visual Computer* 2021).
The most engine-usable dual method in the corpus: CUDA, **lookup-table-free**, asymptotic decider for
ambiguity, halfedge output, quad-only, plus parallel valence-pattern simplification via 5-colour face
colouring. On an RTX 2080 Ti:

| Dataset | DMC | MC |
|---|---:|---:|
| skull 512²×641 | 68 ms | 31 ms |
| torso body 512²×743 | 60 ms | 36 ms |
| skeleton | 84 ms | 42 ms |
| iWP 512³ | 99 ms | 65 ms |

*"DMC is slower than MC, but it still only requires approximately 100 milliseconds to reconstruct surfaces
from large data sets."* Simplification removes ~10% of elements in 11–17 ms. **Critical caveat if you want
triangles:** *"While simplification improves the quality for quads, it is reduced for triangles… If a
triangular mesh is preferred, the 3-X–3-Y pattern simplification should not be applied."* And it is not
topologically correct — sub-cell and two-cell tunnels *"create non-manifold edges… shared by four faces"*.

**Octree LOD transition templates.** `10.48550_arXiv.2401.05984` (HybridOctree_Hex, 2024) is a hex mesher,
but §p.7 is the volumetric analogue of Transvoxel and it's the only explicit seam-template content in the
corpus: *"A transitional scenario arises when the adjacent blocks have level differences. Degenerated
elements appear on the transition faces/edges"*; a face transition resolves to *"one hex, four pyramids,
and four triangular prisms"*, with four enumerated edge-transition configurations.

---

## 4. Family C — voxel and terrain meshing at runtime

The family with the best *production* sources and the worst *academic* coverage.

### Blocky voxels

**Greedy meshing** — only source is the UE5 benchmark in §1. Classic scalar formulation: sweep each of
6 face directions, per 2D slice walk +X for a run of identical voxels, extend +Y holding that width, emit
one quad. Gets you **2.76×** fewer triangles and vertices over face-culled cubes, on top of face culling's
own 49.5×. **Binary/bitwise greedy meshing (64-bit column bitmasks) is not in the corpus at all.**

### Heightfield terrain

| Technique | Source | Speed class | Key number |
|---|---|---|---|
| ROAM bintree split/merge | `10.1109_visual.1997.663860` ⚠️ | **per-frame** | 3,000 tris → ~30 ms/frame on SGI R10000; cost ∝ triangle *changes*, ~few % of mesh |
| View-dependent PM refinement | `10.1145_258734.258843` ⚠️ | **per-frame** | ~224 B/vertex (→140 B optimized); 7.2→14.0 fps; vsplit 2,200 cycles, ecol 4,000 |
| Frostbite quadtree + fixed 33×33 grids | `s2007-advances-course-notes-1-6-mb` | **per-frame** | 4-byte UV per vertex, one shared VB; T-junctions fixed by deleting tris in the finer patch |
| Progressive Buffers (geomorphed cluster LOD) | `s2006-advances-*` | **per-frame** | 16M-tri mesh → 800k tris @ 30 fps; targets 60 MB VRAM; raise LOD >65 fps, lower <55 fps |
| Concurrent Binary Trees (Unity) | `s2021-advances-pdf-5-mb` | **per-frame** | 2×2 km, CBT depth 28, 0.25 m leaves, 256 tris/leaf, **128 MB heap/camera**; 1.4M tris, subdivision 0.03 ms |
| CBT for arbitrary meshes (Intel) | `s2024-advances-pdf-12-5-mb` | **per-frame** | 64–95k active primitives; total update pass **0.122–0.37 ms**; frame total 4.18 ms on ≈PS5-class |
| Low-poly terraced terrain | `10.48550_arXiv.2505.09350` | **bake** | 100k verts → 284.89 ms / 10.6 MB; 500k → 1,707 ms / 106 MB (single-threaded C# in Unity) |

⚠️ Two broken entries in this table, in opposite directions:

- `10.1145_258734.258843` (Hoppe's VDPM, SIGGRAPH '97) — **orphaned Qdrant record**. `distill_search`
  returns live chunks (year 1997, `papers/10/…258843.pdf`), but `catalog_read` → *"No catalog entry found"*
  and `markdown_read` → *"Markdown not found… Check if it has been converted."* It is not in `catalog_list`
  at any offset across 8,648 entries. Searchable, unreadable, uncitable.
- `10.1109_visual.1997.663860` (ROAM) — the catalog title and abstract are correct, but `pdf_path` ends
  `.html`, `file_size_bytes: 3359`, and `markdown_read` returns only *"# digital.library.unt.edu / ##
  Gauging your humanity...This may take some seconds."* — a paywall interstitial. The real ROAM text
  survives only as 26 Qdrant chunks. `catalog_list` reports `converted=false`. **The ROAM numbers above
  came from the index and cannot be re-verified against the document.** Re-download it.

**The CBT line is the best terrain content in the corpus.** `s2021-advances-pdf-5-mb` (Deliot, Yao, Dupuy,
Rijnen — Unity) — a pointer-free bitfield + sum-reduction tree in GPU memory encoding longest-edge
bisection; every active triangle independently decides to bisect/merge via bitwise atomics. No tessellation
shaders, compute + vertex only. The optimization story is the useful part: naive level-2 parent dispatch
touches **2^27 ≈ 127 million nodes** with 16 threads writing the same uint; staging 6-bit values in
groupshared and packing 16×6 bits = 3 uints for one non-atomic write took the sum-reduction from
**5.78 ms → 0.40 ms, 14.5×**, on a GTX 1080 Ti. Stated limits: **one subdivision level per frame** (hurts
fast camera motion), no frustum culling yet, flat triangles still subdivided, each depth level **doubles
both memory and time**.

The Intel follow-up `s2024-advances-pdf-12-5-mb` generalizes CBT from one base triangle to arbitrary
polygon meshes and gets crack-freeness explicitly: *"thanks to the neighborhood information, we can propagate
bisections across multiple triangles to guarantee crack-free surfaces"* (p.51). Code at
`github.com/AnisB/large_cbt`.

### Cluster LOD (Nanite and friends)

`s2021-advances` — Brian Karis, *"A deep dive into Nanite, UE5's new virtual geometry system"*. The build
loop verbatim (p.49):

```
Cluster original triangles
While NumClusters > 1
  · Group clusters to clean their shared boundary
  · Merge triangles from group into shared list
  · Simplify to 50% the # of triangles
  · Split simplified triangle list into clusters (128 tris)
```

The seam-locking chain (pp.35–39) is the transferable insight: naive *"lock the shared boundary edges…
this simplistic idea doesn't work in practice due to cracks"* → group clusters, force them to make the
same LOD decision, *"Now free to unlock shared edges and collapse them"*, and **alternate group boundaries
from level to level**. p.51: *"Merge and split makes this a DAG instead of a tree… Meaning there can't be
locked edges that stay locked and collect cruft."*

Runtime rule (p.68): *"All clusters in group must make same LOD decision. How? Communicate? **No!**
Same input ⇒ same output"* — all clusters in a group store the same unioned error and sphere bounds, so
the decision is local and **the DAG isn't traversed at runtime at all** (p.74).

Shipped numbers ("Lumen in the Land of Nanite", p.157): 433M input triangles → 882M Nanite triangles;
raw 25.90 GB → memory format 7.67 GB → compressed **6.77 GB** → disk **4.61 GB**. That's
**5.6 bytes/Nanite triangle, ~10.9 MB per 1M triangles on disk**. Rasterizes **25M triangles** per frame,
*"consistent throughout the demo, regardless of how complex the scene is"*. LOD target: **1 pixel error**
(2 pixels for shadows).

Unusually candid limitations (p.61): *"Quadrics with attributes mix all errors in one with weights —
**Complete heuristic hack**"*, *"No concept of rate distortion optimization"*. And p.66: *"The DAG ends at
1 root cluster of 128 triangles. At that point cost stops scaling with resolution. It stops scaling all
together"* — tiny instances are the unsolved cost. *"Instances are the new triangles."*

Mobile variant `s2024-advances-pdf-2-6-mb` (Tencent, NanoMesh): same 128-tri clusters, 8-bit indices,
plus an artist-facing merge coefficient `f(level) = floor(pow(base, level))`. Claims **80M triangles at
60 FPS on mobile at ~128 mW**, package ¼ the size of manual LODs. Low-end profile at 116.7k primitives:
culling 2.87 ms, binning 3.25 ms, raster 2.67 ms, **material shading 7.31 ms** of a ~20 ms frame — i.e.
cluster LOD moves the bottleneck to shading rather than removing it.

### Collision and navmesh generation

**Navigation-driven approximate convex decomposition.** `sig2024_Navigation-Driven_Approximate_Convex_Decomposition`
(James Andrews, Epic, SIGGRAPH '24, DOI 10.1145/3641519.3657479). Computes navigable space for a character
of radius r, then decomposes only what's reachable. This is the best collision-proxy source in the corpus:

| Method | V-HACD dataset | PartNet-Mobility |
|---|---|---|
| V-HACD | 10.0 / 25.0 / 98 parts @ 4.5–4.7 s | 10.0 / 25.0 / 99.5 parts @ 5.0–5.4 s |
| CoACD | 31.6 parts @ 34.1 s; 245.3 @ 328.7 s | 15.0 @ 49.0 s; 33.7 @ 64.9 s |
| **This** | **8.4 parts @ 1.1 s**; 40.1 @ 3.9 s | **6.4 parts @ 0.5 s**; 24.4 @ 1.7 s |

Navigable-space overlap at matched settings: **0%** for theirs, 1–85%+ for V-HACD, 38–95% for CoACD.
Also merges kitbashed per-part collision bottom-up — a 7-box railing → **2 shapes**. Directly relevant to
your kitbashing work.

**Real-time fracture via VACD.** `10.1145_2461912.2461934` (Müller, Chentanez & Kim, NVIDIA, TOG 32(4), 2013).
Roman arena, **1M vertices / 500k faces**, destroyed to **20k separate pieces**, *"over 30 fps"* including
rigid bodies, dust and rendering. The trick worth stealing: **"ghost convexes"** with negative volume and
inward normals inserted at overlaps, so artists don't have to weld sub-meshes into a watertight manifold —
without them, overlapping convexes with no coplanar face pair *"fly apart violently as soon as the
simulation starts."*

**Voxel navmesh (Recast-style)** — `GameAIPro2_Chapter32_…` gives the pipeline (voxelize empty space → flag
walkable/nonwalkable/border → discard interior → planar polygons from border voxels → simplify → triangulate)
and real agent parameters (standing radius 0.4 m / height 2.0 m / step 0.5 m / slope 0.5 rad; prone 1.0 /
0.5 / 0.1 / 0.15). `gameaipro1-ch20-…` (FFXIV) gives the shipped scale: 32×32 m tiles, ~4 km² maps,
biggest table ~4 MB for 1.5 km², query ~4 µs, table build <80 s. Both note border simplification is
mandatory — raw voxel borders produce *"an excessive number of small triangles."* Neither reports
incremental rebuild latency.

---

## 5. Family D — unstructured volume meshing (physics, not rendering)

**Every timing in this family is offline.** There is no paper here reporting a per-frame or sub-100 ms
budget. For your engine this means: tetrahedral meshing is an asset-bake step, full stop.

| Algorithm | Stem | Robustness | Time |
|---|---|---|---|
| Shewchuk taxonomy chapter | `10.1201_b11644-11` | — | reference |
| **fTetWild** | `10.48550_arXiv.1908.03581` | **99.97%** of Thingi10k | avg **49.8 s**; 98.7% under 2 min |
| TetGen (as measured by fTetWild) | — | 49.50% | avg 32.3 s |
| CGAL (same) | — | 79.00% | avg 11.7 s |
| Surface chamfering / Delmesher | `sig2024_Soft_Pneumatic_Actuator_Design…` ⚠️ | **all 3,942 valid Thingi10k** | avg **77–82 s**, 637–727 MB |
| WSVM (quality post-pass) | `10.48550_arXiv.2409.05525` | needs good input | <1 min/model |
| CPAFT (parallel advancing front) | `10.48550_arXiv.2405.20618` | 3D complex geometry | 28.0 s → 1.84 s on 32 procs |
| HybridOctree_Hex | `10.48550_arXiv.2401.05984` | closed manifold input only | 218 s (vs 8,175 s prior) |
| HexGen / Hex2Spline | `10.48550_arXiv.2011.14213` | **semi-automatic** | *"favors versatility over efficiency"* |

⚠️ **Stem name is wrong.** `sig2024_Soft_Pneumatic_Actuator_Design_using_Differentiable_Simulation` actually
contains *"Surface chamfering for robust tetrahedral meshing"* (Diazzi, Dai, Panozzo, Attene, TOG 45(4)
Art. 148, 2026, DOI 10.1145/3811395). Fix the catalog entry.

**fTetWild is the one to use.** `10.48550_arXiv.1908.03581` (Hu, Schneider, Wang, Zorin, Panozzo, TOG 39(4)).
Takes arbitrary **triangle soup** — self-intersecting, non-manifold, gapped, duplicated verts — builds a
Delaunay background mesh, inserts input triangles only when insertion doesn't invert elements, improves
with AMIPS-driven split/collapse/swap/smooth. Pure floating point, no rationals. Boundary stays within a
user-set ε-envelope. Real-world case: a Velo3D gyroid exhaust pipe with degenerate triangles and severe
self-intersections, *"cleaned up by our algorithm within 55 minutes… compared to around two weeks of manual
labor."* Also handles an architectural model with **80,999 self-intersecting faces**.
**It doubles as your mesh-repair and robust-Boolean tool**, which may matter more to you than the tets.

The competing framing for slivers is worth knowing: `10.48550_arXiv.2606.14301` ("Taming Slivers", 2026)
argues the *solver* should absorb them, not the mesher — *"more than 90% of slivers can be removed very
efficiently through local topological modifications. However, attempting to eliminate every single sliver
typically requires runtimes far exceeding those needed to generate the initial mesh itself, with no
guarantee of success."* Also: an **isolated** sliver is harmless; **bands/sheets** of slivers cause locking.

**Shewchuk's impossibility results** (`10.1201_b11644-11`, p.8) are the thing to internalize before you
promise anyone a quality bound: a domain with a 1° corner cannot be meshed with all angles >30°, and there
exists *"a domain composed of two polygons glued together that, surprisingly, provably has no mesh whose
new angles are all over 30°."* On hexes (p.29): *"Only a few results are known on guaranteed quality
quadrilateral meshing, and **none for hexahedra**."* And on advancing front (p.9): *"there is no literature
on provably good advancing front algorithms."*

---

## 6. Family E — surface reconstruction (asset ingest)

| Method | Stem | Watertight | Time |
|---|---|---|---|
| **Screened Poisson** | `10.1145_2487228.2487237` | yes | depth 8 **14 s / 133 MB**; David 11.4M pts depth 10 **272 s** |
| SPR + envelope constraints | `10.1111_cgf.14077` | yes | 143 s / 3,740 MB baseline; +8–50% for envelope |
| SSD | `10.1111_j.1467-8659.2011.02058.x` | yes | **275–547 s**, O(N^1.5) |
| Crust (Voronoi filtering) | `10.1145_280814.280947` | **no — hollow, non-manifold** | Bunny 35,947 pts / **23 min** |
| Alpha-shapes + BB patches | `10.1145_218380.218424` | needs α tuned interactively | offline |
| Point-cloud denoising (pre-pass) | `Point-Cloud-Noise-and-Outlier-Removal-…` | n/a | **~30 s** |
| Voxelize + isosurface repair | `10.1109_tvcg.2003.1196006` | **yes, by construction** | rasterization-speed |
| Generalized winding numbers | `10.1145_2461912.2461916` | exact boundary | offline preprocess |

**Screened Poisson is the default.** `10.1145_2487228.2487237` (Kazhdan & Hoppe, TOG 32(3), 2013).
Depth 8–9 in **14–20 s / 133–269 MB** is plausible as an editor preview bake; depth 10 is a minutes-scale
job. Multigrid makes it **linear in octree nodes**; *"increasing the depth by one roughly quadruples the
computation time."* Default α = 4, 1 sample/node. Beats SSD by ~20× on time at equal or better accuracy.
Its own honest failure: with misaligned scans the screening term *"generates a surface that interpolates
these artifacts"* — a *"pock-marked surface that undulates between the two scans."*

**The denoising pre-pass is the highest-leverage 30 seconds in your import pipeline.**
`Point-Cloud-Noise-and-Outlier-Removal-for-Image-Based-3D-Reconstruction` (Wolff et al., Disney/ETH) drops
points that are geometrically or photometrically inconsistent with the *other* views. The concrete win:
unfiltered clouds had to be decimated to fit screened Poisson into **64 GB**; *"we did not have to
downsample the images for our denoised results, allowing us to use the full input resolution."* Effect is
consistent across PSR, SSD and FSSR, and it beats WLOP/EAR/RIMLS because it uses colour.

**Two repair primitives worth lifting wholesale:**

- `10.1109_tvcg.2003.1196006` (Nooruddin & Turk) — voxelize→isosurface round-trip *"produces an everywhere
  manifold"* output. Parity-count with **13 projections** (3 axes + 10 icosahedron normals, majority vote)
  makes the Stanford Bunny watertight. Ray-stabbing is the fallback for double-walled and self-intersecting
  parts. Uses scan conversion, so it GPU-ports naturally.
- `10.1145_2461912.2461916` (Jacobson, Kavan, Sorkine-Hornung) — generalized winding numbers need only
  *"reasonably consistent orientation"*. Their stress case is a game asset in the wild: **3,442 intersecting
  triangle pairs, 1,020 open-boundary edges, 344 non-manifold edges, 67 components**. Their framing is
  yours exactly: *"character meshes and CAD models are often composed of many connected components with
  numerous self-intersections, non-manifold pieces, and open boundaries, precluding existing meshing
  algorithms"* — and such artifacts are *"sometimes purposefully introduced by the designer."*

**Crust is a cautionary tale, not a tool.** `10.1145_280814.280947` was the first *provably correct*
reconstruction and has zero tunable parameters, but: *"it often contains all four triangles of a very flat
'sliver' tetrahedron"*, *"The foot, like all our reconstructions, is **hollow**"*, and *"when the noise level
is roughly the same as the sampling density, the algorithm fails, both in theory and in practice."*

---

## 7. Family F — simplification, remeshing, LOD chains

| Method | Stem | Speed class | Headline |
|---|---|---|---|
| QEM (Garland–Heckbert '97) | `10.1145_258734.258849` | bake | 70k → 100 faces in **15 s**; **10 floats/vertex** |
| Attribute QEM ('98) | `10.1109_VISUAL.1998.745312` | bake | color/UV/normal in one cost |
| Out-of-core vertex clustering | `10.1145_344779.344912` | bake | **100k tri/s**, output-sensitive memory, no disk overhead |
| Progressive Meshes | `10.1145_237170.237216` | bake + **per-frame geomorph** | any face count ±1; vsplit in **5 bits**; V in 30n–37n bits |
| Simplification Envelopes | `10.1145_237170.237220` | bake | **two-sided ε bound**, genus-preserving, self-intersection-free |
| Silhouette clipping | `10.1145_344779.344935` | bake + per-frame | silhouette complexity is **O(√n)** |
| STMW (textured, non-manifold) | `10.48550_arXiv.2409.15458` | bake / bg thread | decimates **every** Thingi10k mesh to **0.1%**; O(n log n) |
| FA-QEM | `10.48550_arXiv.2605.14029` | bake | **10.60 s vs STMW 37.70 s** (3.5×) on Thingi10k |
| Error-bounded feature remeshing | `10.48550_arXiv.1611.02147` | editor-time | Bunny → **5% of input** inside the error bound; 0:28–4:48 |
| CVT multi-facet-clipping remesh | `10.48550_arXiv.2505.14306` | bake, GPU | θ_min 3.44° → 38.87°; **no feature preservation** |
| **Instant Field-Aligned Meshes** | `Instant-Field-Aligned-Meshes` | **<1 s on 100k faces** | 13M-vertex dragon → 80k verts in **71 s**; scales linearly to 100M+ |
| Frame fields | `Frame-Fields-Anisotropic-…` | editor-time | adds density + anisotropy control on top of cross fields |

**Two facts change your simplification design:**

1. **98.9% of ShapeNet is non-manifold** (`10.48550_arXiv.2409.15458` p.2), and repairing to manifold first
   *"may take minutes to hours."* Any simplifier you ship for imported/generated content must eat triangle
   soup. STMW does — simplicial-complex data structure, virtual edges from **triangle-to-triangle** distance
   rather than vertex distance.

2. **The UV-preservation fight is probably not winnable, and the corpus's best answer is to stop fighting it.**
   STMW *abandons UV preservation entirely* and re-bakes texture by mapping each texel back through the
   decimation history (<5 µs/point, trivially parallel). Result at 1% resolution, textured Chamfer ×10⁻¹:
   **STMW 0.10 vs Garland–Heckbert 0.18**. >80% of a user study preferred it. **Cohen et al. 1998
   appearance-preserving simplification — the canonical texture-deviation bound — is not in the corpus.**

**Instant Meshes is the only remesher in the corpus with an interactive budget.** *"Our full pipeline
executes instantly (less than a second) on meshes with hundreds of thousands of faces, enabling new types of
interactive workflows"*; key steps scale linearly, handling *"sizes exceeding several hundred million
elements."* Pure quad by *"optimiz[ing] for a quad-dominant mesh at quarter resolution and subdivid[ing]
once."* If you want a retopo button in the editor, this is the algorithm.

**FA-QEM gives the profile you'll actually hit if you port a simplifier**
(`10.48550_arXiv.2605.14029` p.7, Table 3): quadric construction 25%, priority-queue population 15%,
**collapse loop 55%** (edge pop 5, optimal-position solve 20, area cost 10, neighbor updates 20), texture
bake 5%. The collapse loop dominates, not the bake. And `10.48550_arXiv.2512.19959` ("A Comprehensive Guide
to Mesh Simplification using Edge Collapse", 2025) is a from-scratch porting guide covering exactly the
guards a Rust implementation trips over: *"mesh degeneracies, inverted normals, and improper handling of
boundary conditions."*

**Progressive Meshes still has the cleanest geomorph.** `10.1145_237170.237216` p.3:
`v_j^G(α) = α·v_j^f + (1−α)·v_{A^c(j)}^c`, giving transitions *"without any visible 'snapping'"*. The trick
for discrete per-face attributes (materials, submesh IDs) is that faces missing from the coarse mesh
*"are invisible in M^G(0) because they have been collapsed to degenerate (zero area) triangles."*
Progressive Buffers (`s2006-advances-*` p.11) gives the exact anti-pop condition: *"the geomorph must be
performed at a distance of r away from this transition point, where r is the maximum cluster radius…
so that all vertices have finished geomorphing when the cluster switches LOD."* And the seam recipe (p.17):
when simplifying each cluster, load the adjacent clusters and simplify their **boundary** vertices too while
keeping their interior vertices fixed.

---

## 8. Family G — neural, differentiable, generative meshing

**This family is where the corpus earns its keep for `foundation_vs_slop`, because it contains the
measurements.**

### The topology gap, quantified

`eth-cgl-various-Fru24a` — QUADify (Frühauf, Riemenschneider, Gross, Schroers; ETH/Disney), Table 3,
79 objects from the Myles dataset:

| Method | Chamfer | Aspect ratio >4 | Radius ratio >4 | #Verts |
|---|---:|---:|---:|---:|
| Marching Cubes | 5.22e-5 | **12.02%** | 12.01% | 10.34K |
| DMTet | 5.23 | **16.89%** | 16.32% | 10.92K |
| FlexiCubes | 4.87 | 6.69% | 8.26% | 11.87K |
| QUADify (quad) | 4.96 | **0.00%** | 0.24% | 10.66K |
| QUADify (+displacement) | 4.63 | 0.22% | 0.88% | 42.75K |

And `TetWeave-…` Table 2, self-intersecting faces: **DMTet(128³) 12.351%**, FlexiCubes(128³) 0.203%,
TetWeave **0.000%** at every resolution. QUADify's motivating sentence: nvdiffrec's DMTet meshes
*"contain many skinny, 'sliver' triangles, making them unsuitable for further processing."*

### The production-pipeline audit

`10.55677_ijhrsss_15-2026-vol03i06` — *"Prompted Props, Human Pipelines: Evaluating AI-Generated 3D Assets
for Game-Ready Environments"* (2026). Same stylized tavern built twice:

| | Hand-authored | Hunyuan 3D |
|---|---:|---:|
| Objects | 123 | 103 |
| Triangles | **293,828** | **32,781,505** |
| Logged time | 716 min (11 h 56 m) | 238 min |

**111.6× the triangles with fewer objects.** Time saving 66.8% — *"excluding full retopology and engine
validation."* Decimating a generated boar-head trophy *"began losing triangles, producing holes and damaging
texture alignment"*; UVs were *"fragmented across many small faces or islands… scattered across the texture
space rather than arranged in a way that would allow easy manual editing."* Verdict: viable for ideation,
conditionally viable for static background props, *"not yet reliably viable as drop-in replacements for
human-authored production assets in performance-sensitive, editable, or interaction-heavy game environments."*

That is your thesis with a citation. Its own limitation: single case study, one environment, two tools,
no engine-side frame-time measurement.

### The methods

| Method | Stem | Output soundness | Time |
|---|---|---|---|
| DMTet | `10.48550_arXiv.2111.04276` | 12.35% self-intersecting, 16.89% slivers | 129 ms inference (V100) |
| **TetWeave** | `TetWeave-…` | **watertight + manifold + intersection-free by construction** | 2 min @ 8K pts → <8 min @ 128K |
| QUADify | `eth-cgl-various-Fru24a` | **quad, 0.00% AR>4** | offline inverse rendering |
| BSP-Net | `10.1109_cvpr42600.2020.00012` | **watertight by construction, sharp edges** | **0.5 s/mesh**; avg **654 polygons** |
| SATO (strips as tokens) | `sig2026_Strips_as_Tokens_…` | quad-*dominant*, native UV islands | offline; face budget **[500, 16000]** |
| Occupancy Networks + MISE | `10.1109_cvpr.2019.00459` | over-tessellated MC output | offline |
| VolSDF / MVSDF | `10.48550_arXiv.2106.12052`, `10.1109_iccv48922.2021.00646` | see below | **5.5 h / 20 GB VRAM** per scan |
| CharacterGen | `sig2024_CharacterGen_…` | riggable via A-pose canonicalization | **<1 min** vs Magic123 70 min |
| 2D Gaussian Splatting → TSDF | `10.1145_3641519.3657428` | uniform over-tessellation, no UV/rig | **100× faster** than SDF baselines |
| Neural Progressive Meshes | `10.1145_3588432.3591531` | 400-face base → 25,600 | **6.93 s** per decode |

**BSP-Net is the outlier worth noting.** `10.1109_cvpr42600.2020.00012` skips iso-surfacing entirely —
learns plane equations grouped into convexes via a BSP tree, extracts by classic CSG. Output is
**1,073 verts / 1,910 faces average** where IM-NET₂₅₆ gives **82,965 / 165,929**. Watertight by construction,
sharp features, *"can be easily parameterized"* (UV-able), 0.5 s inference. The cost is representational:
*"can only decompose a shape as a union of convexes. Concave shapes, e.g., a teacup or ring, have to be
decomposed into many small convex pieces, which is unnatural."* Training was **6 days**.

**The hard limit on SDF-based generation, stated plainly** (VolSDF §5, p.9): *"representing non-watertight
manifolds and/or manifolds with boundaries, such as zero thickness surfaces, is not possible with an SDF."*
Cloth, leaves, sheet metal, any single-sided geometry — cannot be represented at all. That is a
structural argument, not a quality argument, and it applies to your voxel/SDF editor too.

**SATO's training filter is an accidental spec for "game-ready".**
`sig2026_Strips_as_Tokens_Artist_Mesh_Generation_with_Native_UV_Segmentation` (TOG 45(4) Art. 75, 2026)
discards non-manifold models, requires face count in **[500, 16000]**, vertex-to-face ratio ≤ 1.0
(*"models violating the latter criterion are often highly fragmented and close to a triangle soup"*), and
10–300 UV islands. Its user study — **25 3D-industry professionals**, top-3 scoring — put SATO at 2.61 vs
BPT 1.4, DeepMesh 1.17, MeshAnythingV2 0.18. Honest limit: quad output is *"predominantly quad-dominant"*,
and *"when a strip has an odd length or contains repeated vertices, local faces may degenerate into triangles."*

---

## 9. What the corpus does NOT have

### 9.0 First: search cannot see 342 of your documents

`catalog_list(embedded=false)` returns **471 of 8,648** entries. Of those:

- **342 have `converted=true` with `embedding_skip: {reason: "zero_chunks_or_empty"}`** — the markdown
  exists and `markdown_read` works, but there are **zero Qdrant chunks**, so `distill_search` cannot return
  them at any score, for any query.
- **129 have `converted=false`** — neither readable nor searchable.

Two of the 342 are papers this document originally listed as missing:

| Stem | What it actually is |
|---|---|
| `10.1145_566570.566586` | **Dual Contouring of Hermite Data** — Ju, Losasso, Schaefer & Warren, SIGGRAPH 2002 |
| `10.1145_344779.344899` | **Adaptively Sampled Distance Fields** — Frisken, Rockwood, Perry & Jones, SIGGRAPH 2000 (707 citations) |

Other meshing-relevant papers in the invisible set, worth knowing you own:

| Stem | Paper |
|---|---|
| `10.48550_arXiv.2404.15661` | **CWF: Consolidating Weak Features in High-quality Mesh Simplification** (2024) — normal-anisotropy + CVT energy with decaying weight; aligns the *weak* features QEM destroys. 100 CAD models from ABC + 21 organic. |
| `Dev2PQ-Planar-Quadrilateral-Strip-Remeshing-of-Developable-Surfaces` | **Dev2PQ** — Verhoeven, Vaxman, Hoffmann, Sorkine-Hornung, TOG 2022 — curvature-aligned planar-quad strip remeshing |
| `10.1109_CVPR46437.2021.01120` | **Neural Geometric Level of Detail** — Takikawa et al. 2021 — sparse-octree neural SDF with continuous LOD |
| `10.1109_CVPR.2019.00025` | **DeepSDF** — Park et al. 2019 |
| `10.1007_s00366-024-02023-w` | **CBC3D image-to-mesh conversion** (2024) — adaptive body-centred-cubic tet meshing from segmented images |
| `10.1145_360767.360802` | Sutherland & Hodgman, *Reentrant polygon clipping*, CACM 1974 (the clipper MC's solid-modelling section calls) |

And four are **doubly broken** (`converted=false`, so no markdown and no index) — notably
`10.1145_1186822.1073278` **Cache-oblivious mesh layouts**, which is directly relevant to the vertex-cache
problem the Dreams deck describes.

**Practical rule going forward: `distill_search` is a recall tool, not a coverage tool. Before concluding
the corpus lacks something, enumerate `catalog_list`.** Running `distill_reindex` on the 342 would fix this
at the root.

### 9.1 Genuinely absent — re-tested by catalog enumeration, not search

**Blocking for a voxel editor:**

1. **Transvoxel** (Lengyel 2010, "Transition cells for dynamic multiresolution marching cubes") — zero
   stems matching `transvoxel` or `lengyel`; cited once, as reference [24] in `10.1007_s13173-012-0097-z`.
   **This is now the largest single gap for chunked voxel terrain**, though note §3.1: DC's minimal-edge
   rule already gives you seam-free octree transitions if you go the DC route rather than MC.
2. **Manifold Dual Contouring** (Schaefer, Ju & Warren, TVCG 13(3), 2007) — `10.1109_tvcg.2007.*` holds
   only `.39` and `.42`; canonical `10.1109/TVCG.2007.1012` absent. **Intersection-free Contouring on an
   Octree Grid** (Ju & Udeshi) likewise absent. These are the two fixes the Dreams deck applies and reports
   still don't compose — you have the post-mortem but not the methods.
3. **Surface Nets** (Gibson 1998, either the VVS'98 *"Using distance maps…"* or the constrained-elastic
   variant) — no `gibson` stem, no matching DOI block. The only Surface Nets data in the corpus is one row
   of the UE5 benchmark.
4. **Extended Marching Cubes** (Kobbelt, Botsch, Schwanecke, Seidel, SIGGRAPH 2001, pp. 57–66) — the
   SIGGRAPH 2001 block `10.1145_383259.*` holds `.383287/.383288/.383292/.383300/.383309/.383317`;
   canonical `.383265` absent. **Kizamu** (Perry & Frisken 2001, `.383285`) also absent.
5. **Binary/bitwise greedy meshing** — no source. Only the classic scalar formulation.
6. **Geometry clipmaps** (Losasso & Hoppe 2004) — `10.1145_1015706.*` holds `.1015710` (perceptual audio)
   and `.1015784` (hair geometry); `.1015799` absent. **Geomipmapping** (de Boer) and **chunked LOD**
   (Ulrich 2002) likewise absent.
7. **Cubical Marching Squares** (Ho et al. 2005) — `10.1111/j.1467-8659.2005.00842.x` absent.

**Blocking for asset ingest:**

8. **Curless & Levoy 1996 (TSDF) / KinectFusion / voxel hashing** — nothing on volumetric fusion at all.
   Near-miss worth noting: `10.1145_237170.*` holds `.237199` (Light Field Rendering), `.237200` (The
   Lumigraph), `.237216` (Progressive Meshes), `.237220` (Simplification Envelopes), `.237270` (Fitting
   Smooth Surfaces) — but **not `.237269`**, which is Curless & Levoy. Off by one.
9. **Ball-pivoting** (Bernardini et al. 1999) — `10.1109_2945.*` holds `.485620`, `.675649`, `.764870`;
   `.817351` absent.
10. **Original Poisson Surface Reconstruction** (Kazhdan, Bolitho & Hoppe 2006) — no `10.2312_sgp*` stem.
11. **Cohen et al. 1998 Appearance-Preserving Simplification** — `10.1145_280814.*` holds exactly one
    entry, `.280947` (the Crust); canonical `.280832` absent.
12. **Botsch & Kobbelt incremental isotropic remeshing** — cited only.

**Three conflation traps.** All three of these ARE in the corpus and are easy to mistake for one of the
absent papers above:

- `10.1145_237170.237220` = Cohen et al., **Simplification Envelopes**, SIGGRAPH **1996** — *not*
  Appearance-Preserving Simplification (1998).
- `10.1109_VISUAL.1998.745312` = Garland & Heckbert, **Simplifying surfaces with color and texture using
  QEM**, VIS 1998 — same year, same problem, different paper from Cohen 1998.
- `10.1145_218380.218424` = Bajaj, **Bernardini**, Xu, *Automatic Reconstruction of Surfaces and Scalar
  Fields from 3D Scans*, SIGGRAPH 1995 — Bernardini is an author, but this is alpha-shapes, **not**
  ball-pivoting.

### 9.2 Missing measurements (not missing papers)

- **Re-mesh latency after an edit** for a *blocky* voxel world. The one edit-latency figure anywhere in the
  corpus is DC's ~30 ms CSG sphere on a 256³ signed octree. Nothing measures ms-to-remesh-a-chunk after a
  block place/destroy; the closest is the UE5 paper's *initial generation* stalls (461–2,357 ms).
- **Per-chunk memory accounting** for a blocky voxel world — voxel storage vs mesh storage, palette
  compression, RLE. Nanite gives bytes/triangle; DC gives MB per model; nothing gives bytes/chunk.
- **Incremental navmesh rebuild timing** after a runtime geometry change.
- **Nanite build times.** p.61 asserts *"Import and build time matters too"* with no seconds-per-asset figure.
- **Anything Bevy-, Rust-, or wgpu-specific**, and no meshoptimizer benchmark data.

### 9.3 Catalog defects to fix

- `sig2024_Soft_Pneumatic_Actuator_Design_using_Differentiable_Simulation` actually contains **"Surface
  chamfering for robust tetrahedral meshing"** (Diazzi, Dai, Panozzo, Attene, TOG 45(4) Art. 148, 2026).
  Wrong stem entirely.
- Wrong years in the index: `s2007-advances-course-notes-1-6-mb` → **1942**;
  `s2007-advances-pdf-slides-14-8-mb` → 1978; `Instant-Field-Aligned-Meshes` → 2006 (actually 2015);
  `Frame-Fields-…` → 2010; `Iso-Points-…` → 1992; `10.1016_j.cag.2006.07.021` → 1987 (actually 2006);
  `10.1109_visual.1997.663860` (ROAM) → 2002-11-23. **Year filters on `distill_search` are unreliable
  across this whole topic — don't use them.**
- `catalog_read` returns no title for ~35 of the 69 stems in this document — essentially every
  non-arXiv/non-Crossref stem (`10.1145_*`, `10.1016_*`, `s20xx-advances-*`, every `sig20xx_*`). All titles
  here were read out of page 1 of the markdown. Worth a `catalog_backfill_title` pass.

---

## 10. Actions

**Re-index (free — you already own these):**

1. `distill_reindex` the **342 zero-chunk documents**. Dual Contouring and ASDF alone justify it; there are
   likely more meshing papers in the set that my keyword filter missed.
2. Re-convert `10.1145_258734.258843` (Hoppe's VDPM) — orphaned index record, no catalog entry.
3. Re-download `10.1109_visual.1997.663860` (ROAM) — current PDF is a 3.3 KB paywall interstitial.
4. Re-download the four `converted=false` entries, especially `10.1145_1186822.1073278` **Cache-oblivious
   mesh layouts**.
5. `catalog_backfill_title` across the non-arXiv stems.

**`paper_download` shortlist, in priority order:**

1. **Lengyel (2010), "Transition cells for dynamic multiresolution marching cubes"** — Transvoxel. The one
   genuine blocker for crack-free chunk LOD if you stay on MC.
2. **Schaefer, Ju & Warren (2007), "Manifold Dual Contouring"** — DOI 10.1109/TVCG.2007.1012, plus
   **Ju & Udeshi, "Intersection-free Contouring on an Octree Grid"**. You have the Dreams post-mortem of
   applying both; you don't have either method.
3. **Kobbelt, Botsch, Schwanecke & Seidel (2001), "Feature Sensitive Surface Extraction from Volume Data"** —
   Extended MC. DC positions itself explicitly against this; you only have one side.
4. **Gibson (1998), Surface Nets** — the cheapest smooth voxel mesher, and you have one table row on it.
5. **Botsch & Kobbelt (2004), "A remeshing approach to multiresolution modeling"** — the portable
   editor-side remesher.
6. **Losasso & Hoppe (2004), "Geometry Clipmaps"** — only if terrain stays heightfield-based anywhere.

**Read first, before writing any mesher code:** `10.1145_566570.566586` §2.3 (QEF/QR representation) and
§3.2 (the minimal-edge rule). Those two subsections are the difference between a DC implementation that
works and one that produces 2× the polygons with cracks.
