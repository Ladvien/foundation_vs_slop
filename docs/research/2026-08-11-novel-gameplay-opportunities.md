# Meshing opportunities → gameplay nobody has shipped

**Date:** 2026-08-11
**Basis:** 8,920 documents / 275,278 chunks after the four-track sweep. Every row traces to a measured
figure or a stated theorem; the "unproven" column says where it doesn't.

---

## The table

| # | Opportunity | What's actually novel | **What it looks like in the game** | Evidence | Still unproven |
|---|---|---|---|---|---|
| **1** | **Sub-voxel carving** — encode surfaces by integer edge-intersection counts instead of vertex signs | "Puts no lower bound on the size of features resolvable on a fixed grid" — sidesteps Nyquist. Provably manifold *and* intersection-free (primal path) | You carve **letters into a stone wall and read them**. A wire, a crack, a chain link thinner than a voxel survives. The grid stops being the aesthetic. | Subgrid MT: 180³ ≈ 500³ classic, ~7× fewer triangles, Thm A.1 + D.1 | Needs all-roots-along-edge finding; sharp-feature path loses the intersection-free proof |
| **2** | **Editable at render distance** — field-derived LOD where repair is a mip filter | Aokana renders **10¹⁰ voxels at 6 ms** with 5% resident — but is explicitly *not* editable. Nobody has closed that gap. | Stand on a peak, see a valley **5 km out**, fire into it and watch the crater form — at distance, without loading it. Not "deform radius 40 m." | Aokana: 6 ms @ 64K res, 9× VRAM cut, RTX 3060 Ti. Editing deferred to future work | Aokana's DAG shares subtrees, so a write edits everywhere; needs a non-shared hot tier |
| **3** | **Hundreds of concurrent diggers, no authority server** | Edit log as a join-semilattice: **4.8 MB for 100k ops vs 1.7 GB** per-voxel. CALM gives monotone ⟺ coordination-free as an *equivalence* | 200 players excavate one mountain simultaneously. No server arbitration, no rubber-banding, no rollback. Everyone's holes are everyone's holes. | CALM (`1901.01930`); TSDF fold is a commutative monoid; i32 fixed-point makes it bit-exact | Dig and deposit don't commute *semantically* — either go dig-only or accept intent loss under a total order |
| **4** | **Surface authoring that survives destruction** | Common subdivision + L²-nearest attribute transfer. The shared tet grid **is** the common refinement, so the expensive half is free | You spray graffiti on a wall, then blow a hole through it. The paint on the remaining wall is still **exactly where you sprayed it** — not smeared, not reset. Blood, scorch, moss, wear all persist through carving. | Integer Coordinates §3.8, §4.4, prefactorable transfer | Improvement may be confined to the brush band and be small — one afternoon to falsify |
| **5** | **Runtime fragments that are correct physics bodies** | Guaranteed intersection-free extraction (the clamp) feeds convex decomposition that can't fail | No pre-fractured props. Carve *any* shape and it becomes a correct rigid body immediately — a spiral, a hollow shell, a letter. Chunks you invent, not chunks the artist authored. | CoACD: **49% → 80%** RL manipulation success vs V-HACD, intersection-free hulls by construction | CoACD is 1–4 min/part offline; needs a real-time variant or aggressive caching |
| **6** | **The world is a seed plus a 5 MB file** | Deterministic fold over a grow-only op log; i32 fixed-point gives bit-exact cross-machine replay | Share a fully excavated world as a file **smaller than a screenshot**. Scrub the world backwards through time. Watch a ghost replay of how someone dug their base. | 100k ops × 48 B = 4.80 MB; 55 distinct f64 sums over 200 shuffles vs **1** in i32 | Out-of-order arrival forces a re-fold from checkpoint; needs checkpoint cadence measured |
| **7** | **Physics straight against the field** | Collide against the SDF; never build a collider mesh | Destruction and physics are **simultaneous**. No frame where the hole is visible but not yet solid. Debris interacts the instant it exists. | KinectFusion §5: thousands of particles simulated directly on the TSDF | No game-scale demonstration; character controllers against an SDF are unproven at speed |

---

## Why each is actually unoccupied

**1 — Sub-voxel carving.** Every shipped voxel game makes the grid the art direction because it has no
choice: classic marching infers crossings from vertex signs, so anything below one cell is invisible. Subgrid
MT removes that floor. Teardown's voxels are visible on purpose; Minecraft's are the identity. Nobody has a
smooth destructible world where the grid is *undetectable* because features finer than it survive.

**2 — Editable at render distance.** This is the sharpest gap in the whole review, because half of it is
solved and published. Aokana renders tens of billions of voxels at 6 ms on a 3060 Ti with only ~5% resident,
and then says, in Future Work: *"refer to the implementation in HashDAG, using persistent data structures to
support interactive modifications."* Three structural blockers — distant chunks resident only at aggregate
LOD (at LOD 5, one resident voxel is 32,768 source voxels), the LOD chain built offline, and DAG subtree
sharing meaning a write hits every location that shares the node. Every shipped destructible game caps deform
range far inside draw range. Nobody has made them equal.

**3 — Concurrent diggers.** The blocker is not networking, it's algebra. The TSDF update is a commutative
monoid but **not** idempotent, and the weight clamp everyone ships to fit `W` in a `u8` breaks commutativity
outright — 246 distinct results across 5,040 orderings. Move the lattice to the edit log and the voxel field
becomes a derived cache; then CALM says coordination-freedom is available exactly when the op set is
monotone. That's a *theorem*, not a heuristic, and it's why dig-only worlds get it free.

**4 — Persistent surface authoring.** Every sculpting tool ships nearest-point copy for attribute transfer,
which smears at exactly the boundary the player was looking at. The L² formulation exists and is
prefactorable, and in a voxel pipeline the common refinement is free because both meshes were cut by the same
grid. This is the cheapest genuinely-new row in the table.

**5 — Runtime fragments.** Games pre-fracture because runtime convex decomposition is unreliable — and the
measured reason is input quality: CoACD assumes a 2-manifold solid. Guarantee intersection-free extraction and
the decomposition stops failing. The 49% → 80% figure is manipulation success, not cosmetics: collision-proxy
quality decides whether interaction *works*.

**6 — World as a file.** Minecraft schematics exist; time-scrubbing a world's full excavation history as a
first-class mechanic does not, because nobody's terrain state is a deterministic fold over an op log.

**7 — SDF physics.** Demonstrated in research eight years ago, never in a game, and it removes the entire
collider-rebuild category — which the analysis suggests may dominate mesh time anyway.

---

## The one architectural decision underneath five of these rows

Rows 2, 3, 5, 6 and the no-session-degradation property all depend on the same choice: **derive LOD from the
field, not from the mesh.**

The theory now says why, precisely. Edit-proportional hierarchy repair requires the grouping rule to carry a
**local validity certificate**, and it exists in exactly two forms:

- **Canonical by construction** — an octree's subdivision is a pure function of coordinates, so no
  rebalancing is needed and *"the tree after updates is same as the one built from scratch."* Exact
  from-scratch identity, for free.
- **Locally certifiable** — DynHAC replaces "the best merge" with "any (1+ε)-good merge," a predicate
  checkable from a vertex's own incident edges, which is what yields its 4-hop dirty-set bound.

A Nanite-style DAG has **neither**. Its grouping is a global METIS partition, so no local predicate can
certify it, the computation distance is unbounded, and multilevel refinement rewrites the same locations
repeatedly — violating the write-once model the change-propagation framework requires. DynHAC is the sharpest
evidence: it is the closest structural analogue, and its solution was to **abandon the global optimum**.

So the trade is explicit. Keep mesh-space simplification and you keep Nanite-quality silhouettes and lose
edit-proportional repair. Go field-derived and you lose organic simplification — distant silhouettes are
measurably blockier — and gain rows 2, 3, 5, 6 plus a world that looks the same at hour eight as at minute
one. Given that nobody has made the mesh-space path work incrementally, and two independent lines say why it
can't be made to, that's the trade to take.

Caveat worth carrying: DynHAC's speedup over rebuild ranges from **423× to 1.56×** depending purely on how
local the dirty set happens to be. Locality is data-dependent, not guaranteed — which is why E1 (measure the
early-cutoff hit rate) is still the first thing to run.

---

## Order of operations

1. **E1** — hash each cluster slab, log what fraction actually changes per brush stroke. Hours. Tells you
   the ceiling on every row above.
2. **Clamp the QEF minimizer** to (1−ε) inside its cell and re-measure intersections per 1,000 triangles.
   Half a day. Settles whether row 5 is already available.
3. **Row 4** — the attribute-transfer test. One afternoon, one model, falsifiable.
4. **Row 3's algebra check** — 8 brush ops, 40,320 orderings, count distinct bricks. Two hours. Tells you
   whether you already lost commutativity.
5. Then the architectural decision on field- vs mesh-derived LOD, because rows 2, 3, 5, 6 hang off it.
