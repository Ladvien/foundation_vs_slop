# Speed on the best meshing algorithm — what the corpus actually says

**Date:** 2026-08-11
**Question asked:** "What about speed on the best mesh algorithm? Anything in there?"
**Basis:** normalized speed sweep across the corpus + verification pass on four second-hand figures.
Every number below is from a paper in home-still with the DOI attached. Where a number is
second-hand or unverifiable, it says so.

---

## The one-line answer

**Yes — and the finding is that the algorithm stopped being the dominant term.**

Two measurements make this concrete, and neither one changed a line of the extraction math:

| Change | Effect | Source |
|---|---|---|
| Same Marching Cubes, compute shader → mesh shader | **114.2 → 2679.4 fps (23.4×)** | Elliott MSc, Waikato 2022 |
| Same CBT sum-reduction, atomics → staged through LDS | **5.78 → 0.40 ms (14.5×)** | Unity SIGGRAPH 2021 talk |

For comparison, the entire measured spread *between* extraction algorithms — MC vs Dual MC vs
FlexiCubes — is **1.5× to 3.9×**. Where you run the work is worth roughly an order of magnitude more
than which extractor you run.

The Elliott result names the mechanism: the compute-shader version is bound by writing vertices out
to VRAM and reading them back; the mesh-shader version never leaves shared memory. That is a data
*movement* win, not an arithmetic win.

---

## 1. Raw throughput ranking (comparable class: uniform grid, GPU, per-voxel)

**Marching Cubes is still the fastest thing measured.**

- **5.42 G voxel/s, 330 M tri/s** on an RTX 2080 Ti — Grosso & Zint, `10.1007/s00371-021-02139-w`
- Dual Marching Cubes costs **1.52–3.50×** MC on the same hardware and code base
- FlexiCubes costs **2.77–3.92×** MC forward-pass

So "MC is fast, duals cost you 2–4×" survives. But three caveats gut most of the conclusions people
draw from it:

**(a) The 64³ numbers are mostly launch overhead.** Fitting a linear model to FlexiCubes' own MC
timings across resolutions gives a fixed cost `a ≈ 1.88 ms` and a marginal rate of ~655 M voxel/s.
At 64³, **73% of the reported time is not meshing.** Any cross-paper normalization that uses small-grid
numbers is measuring dispatch latency.

**(b) The reproducibility floor is ~1.5× — in opposite directions.** TetWeave re-measured FlexiCubes
at 128³ and got **9.63 / 15.25 ms** where FlexiCubes reported **14.06 / 9.53 ms** for the same code.
That is a 1.46× and 1.60× disagreement, and they disagree in *opposite* directions on the two
columns. Treat any published cross-paper speed ratio below ~2× as noise.

**(c) GPU MC throughput has not tracked GPU capability.** From a GTS 450 to a 2080 Ti is **10.7×
more memory bandwidth** and bought only **~1.7× more MC throughput**. Eleven years of hardware,
1.7×. That is the signature of a workload bottlenecked on something the hardware stopped improving —
which is exactly what the mesh-shader result fixes.

---

## 2. The elephant: contouring is under half the cost of a usable mesh

Grosso & Zint Table 5, same paper, same run:

| Stage | Time |
|---|---|
| Contouring | 68 ms |
| Halfedge construction | 58 ms |

**Extraction is 54% of getting to a mesh you can actually use** — and that's before collider
generation, before normals, before upload. Optimizing the contour to zero would buy you at most 1.9×.

It gets worse when the grid isn't uniform. **TetWeave Table 3**: the Delaunay tetrahedralization to
Marching Tets ratio ranges **15.3× to 81.5×** — contouring is **1–2% of the pipeline**. Anyone
optimizing the extractor in an unstructured pipeline is polishing 2% of the runtime.

And in the real-time volumetric line, the two independent measurements agree:

- **Dong 2018**: meshing is **76.5–89.6% of the pipeline** (confirmed; caveat — camera poses are
  precomputed, so tracking isn't in the denominator, which inflates the share)
- **nvblox**: meshing is the *least* GPU-accelerable stage — **×3–13** speedup vs TSDF fusion's
  **×174–177**

That's the tell. Fusion parallelizes beautifully; meshing doesn't, because it's irregular
allocation-bound work. Which is the same diagnosis as the 1.7×-in-eleven-years number.

---

## 3. The regime shift — verified

Four figures I'd cited second-hand, now checked against sources:

| Claim | Verdict | Source |
|---|---|---|
| Meshlet compression: 15.5 M tri in 0.59 ms (RX 7900 XTX) | **CONFIRMED** | `10.2312/vmv20241204` |
| Work graphs: 79,710 instances in 3.74 ms | **CONFIRMED** | `10.1145/3675376` |
| CBT: <0.2 ms on console hardware | **CONFIRMED** | `10.1145/3675371` |
| CBT: 5.78 → 0.40 ms from Dupuy 2020 | **numbers right, attribution wrong** — it's the Unity SIGGRAPH 2021 talk | — |

**What actually changed is the ordering of what matters.** The modern ranking of optimization levers,
by measured effect size:

1. **Stage placement** (which shader stage the work lives in) — 23.4×
2. **Data layout / where results land** (LDS vs VRAM) — 14.5×
3. **Primitive-count regularity** (does your output size vary per cell)
4. **Per-cell arithmetic** (the thing every extraction paper optimizes) — 1.5–3.9×

Every isosurface paper in the corpus optimizes #4. Almost none of them touch #1.

**Work graphs are two-sided, though.** An independent profile shows them **2.8–3.4× slower** on
classification workloads. They win on deeply irregular, dynamically-expanding work; they lose on
"look at every cell once." Don't assume.

---

## 4. The opening: isosurface extraction inside a mesh shader

This is the actionable item, and it's already published three times:

- **Kreskowski et al., CGF 2022** — `10.1111/cgf.14670`
- **Elliott, MSc thesis, Waikato 2022** — the 23.4× number
- **Nishidate & Fujishiro, I3D 2024** — `10.1145/3651285`

The structural fit for your engine: a mesh-shader chunk mesher **never persists a vertex buffer**.
Cell classification, vertex placement, and triangle emission all happen in one dispatch, in shared
memory, and the primitives go straight to the rasterizer. That deletes the halfedge/upload half of
Grosso's Table 5 for the *render* path entirely.

The catch — and it's a real one for a destructible-terrain engine — is that you still need the mesh
on the CPU side for colliders. So the mesh-shader path is a win for the visual mesh and does nothing
for the physics mesh. Which is precisely the argument for **collide against the SDF directly** (row 7
in the opportunities table): if physics never needs a mesh, the mesh never needs to exist in memory.

Those two findings compose. Alone, each is a decent optimization. Together they eliminate the
persisted-mesh stage from the engine.

---

## 5. What Media Molecule found, which inverts the folklore

Textbook advice is "MC is the simple fast one, DC is the complicated one." Media Molecule's
production experience says the opposite on GPU: Dual Contouring is **"easy on GPU"** because it emits
exactly one vertex per cell, while Marching Cubes' variable output **"makes for asymmetrical code."**

That's lever #3 above — primitive-count regularity. On a 2011 GPU, MC's arithmetic simplicity won.
On a modern one, DC's *output* regularity is worth more. This is the single most likely place where
the corpus's speed ranking is stale.

---

## 6. What nobody measured

The honest part. These gaps mean the "best algorithm" question is partly unanswerable from the
literature:

1. **No 2020+ paper benchmarks MC vs Surface Nets vs Dual Contouring against each other.** That
   comparison does not exist. Every "X is faster" claim in blog posts traces to different hardware,
   different grids, different years.
2. **Surface Nets and greedy meshing have no credible published timings at all** — despite being the
   two things game engines actually ship.
3. **Extraction → renderable end-to-end** is never measured. Papers stop at "triangles produced."
4. **Amortized cost per frame under editing** — the only number that matters for a destructible game —
   appears nowhere. Everything is one-shot full-volume extraction.
5. Collider generation time is never in any denominator.

Items 3, 4 and 5 are why the speed literature can't tell you what to build. It measures a workload
that isn't yours.

---

## What I'd actually do with this

**Stop optimizing the extractor.** The measured ceiling is 1.5–3.9× and the reproducibility floor is
1.5×, so much of that range isn't real.

**Ranked by expected payoff:**

1. **Port the chunk mesher to a mesh shader.** Published three times, 23.4× measured, and it deletes
   the vertex-buffer round trip. Highest-confidence win available.
2. **Measure your own MC vs Surface Nets vs DC.** The comparison doesn't exist in the literature and
   you have the only implementation where it's an apples-to-apples question. This is a publishable
   result in itself, and it's a day of work.
3. **Measure amortized cost per edit, not per volume.** Gap #4. Your workload is "re-mesh the 3% of
   cells a brush touched," and nothing in the corpus measures that regime.
4. **Time your collider path against your contour path.** Grosso says post-contour work is 46% on a
   uniform grid with no physics. Yours is almost certainly worse, and if it dominates, the SDF-physics
   row goes from "interesting" to "the main event."

The through-line: every measurement above says the win is in **what stage the work runs in and what
it writes**, not in which extraction rule you pick.
