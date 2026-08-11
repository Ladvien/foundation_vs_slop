# Importing math from adjacent fields — three lifts, audited

**Date:** 2026-08-10
**Question:** can the "lift a defect into abstract math, then search for a named structure with that
property" move actually produce something, or is it just a nice story about Subgrid MT?
**Method:** three meshing defects stripped of graphics vocabulary, restated as properties of an invariant,
searched against the corpus + Crossref/OpenAlex/S2, then audited for transfer conditions and given a
constructive test (can you write it down for one cell?).
**Result:** one clean NO, two YES-with-caveats, one of which is directly implementable.

---

## 0. The pipeline

1. **State the defect in graphics vocabulary.**
2. **Climb three rungs**, forbidding graphics words at the top. Defect → geometric property → algebraic
   or topological property → *name of the invariant*.
3. **Search the abstract statement**, not the application. This is where the math corpus earns its keep.
4. **Transfer-condition audit.** What does the theorem assume? Which assumption does a voxel grid
   violate? Fatal or repairable? *This kills most candidates and is the step that separates the method
   from numerology.*
5. **Constructive test.** Write the object down for a single 2×2×2 cell. If you can't in an afternoon,
   it doesn't transfer.

The value is in step 4. A method that always answers "yes, transfers!" is worthless.

---

## 1. Cracks between LOD chunks → sheaf cohomology

**The lift:** *"independently computed local decompositions must agree on their overlaps and assemble into
one globally consistent object, with a computable obstruction that vanishes exactly when they do."*
→ the sheaf gluing axiom; obstruction = Čech H¹; computable version = **cellular sheaves and sheaf
Laplacians** (Hansen & Ghrist, `10.1007/s41468-019-00038-7`; Curry, Ghrist & Nanda,
`10.1007/s10208-015-9266-8`).

### Verdict: DOES NOT TRANSFER

The reason is precise and worth keeping. Cellular sheaf cohomology assumes the restriction maps
`ρ_{σ⊴τ}` are **fixed data of the sheaf, chosen before you look at the field**. In LOD chunk meshing there
is no restriction map until you decide at which octree level the shared face is discretized — and *making
that decision is the crack fix*. So a crack is a failure of the **functoriality** axiom, not the **gluing**
axiom, and H¹ is defined only on objects that already satisfy functoriality.

Both ways of forcing the sheaf into existence collapse:

- **Coarse-level face stalk, push forward from the fine side** — definable but not injective, so `δ⁰x = 0`
  no longer implies crack-free. The certificate becomes *unsound*.
- **Fine-level face stalk, prolong from the coarse side** — no canonical linear prolongation exists.
  Splitting a coarse edge in two, *which* fine edge carries the vertex depends on the interior sample,
  **which the coarse chunk does not have.**

Two secondary strikes. H¹ answers *"does some crack-free assignment exist?"*; the engineering question is
*"does this assembly glue?"*, which is membership in `H⁰ = ker δ⁰` — an O(boundary) union-find test needing
no cohomology. And in the only regime where the linear algebra is cheap (identity-block restrictions →
`δ⁰` is a graph incidence matrix), **H¹ is identically zero.** The cheap case is the useless case.

### What it produced anyway

**Lengyel already wrote the cocycle condition, procedurally.** From the transition-cell paper
(`10.1080_2151237x.2011.563682`, now in the corpus):

> "Any open edges placed on a lateral face must be matched by an edge having the same endpoints on the
> coincident lateral face of an adjacent transition cell, and this edge must have the **opposite winding
> direction**."

That is `δ¹ = 0` for an oriented cellular sheaf whose lateral-face stalk is a 5-element set, verified by
hand across 512 cases collapsed to 73 equivalence classes. The formalism adds vocabulary, not capability.

**Two things to do regardless, both forced out by the audit:**

1. **Quantize boundary vertex positions to a shared, level-independent integer lattice.** Without exact
   representability there is no certificate of any kind — `ker δ⁰` over ℝ is measure-zero, so every
   configuration reports "crack." Different chunk origins guarantee last-ULP disagreement.
2. **Assert `∂₂[M] = 0`.** Hash every directed edge of the assembled mesh; every undirected edge must
   appear exactly twice with opposite orientation. O(#triangles) with a hash map. This is a **cocycle**,
   not a cohomology class — which is exactly right, because you're certifying a given assembly rather than
   asking whether some assembly exists.

**Cheapest experiment (under a day):** machine-check Lengyel's 512-case table as an F₂ cocycle condition.
Transcribe the table, compute the open-edge set on each of the 4 lateral faces per case, enumerate all
face-adjacent pairs agreeing on the 3 shared samples, assert opposite winding. ~10⁶ comparisons,
milliseconds, no linear algebra. If it passes, sheaves told you nothing. **If it fails you've found a table
bug that 15 years of visual inspection missed.**

---

## 2. Order-independent parallel edits → join-semilattices / CRDTs

**The lift:** *"shared mutable state, many uncoordinated writers, final state independent of arrival order
and of duplicate delivery, mergeable without consensus — while remaining a faithful numerical estimate."*
→ state-based CRDTs and the join-semilattice requirement (Shapiro et al.,
`10.1007/978-3-642-24550-3_29`); **CALM / Bloom^L** (Hellerstein & Alvaro, `10.1145/3369736`; Conway et al.,
`10.1145/2391229.2391230`).

### Verdict: TRANSFERS WITH CAVEATS — and the interesting findings are on both sides of the one I predicted

**The TSDF update is a commutative monoid, not a join-semilattice.** Change coordinates to `S = W·D` and
Curless & Levoy's eq. (3)/(4) become plain vector addition: `S ← S + w·d`, `W ← W + w`. Identity `(0,0)` is
exactly the paper's own "unseen" state — which is why `W = 0` has to be special-cased in extraction.
Not idempotent: from `(D,W) = (0.30, 2.0)`, applying `(d,w) = (−0.10, 1.0)` once gives `D = 0.1667`, twice
gives `D = 0.1000`. Duplicate delivery doesn't just inflate confidence, it **re-weights the estimate**.

There's a trap: on a *fresh* voxel (`W = 0`) duplication leaves `D` unchanged, so it looks idempotent until
a second distinct brush touches the voxel. Worst possible shape for a bug.

### The finding to act on tonight

**The variant everyone ships is worse — it isn't even commutative.** KinectFusion's eq. 13 clamps the
weight, `W ← min(W + w, Wη)`, to keep it in a `u8` and to get recency bias. Once `W` saturates this
degenerates to an exponential moving average, which is order-dependent *by construction*. Enumerating all
5040 permutations of 7 observations:

| `Wη` | distinct `D` over 5040 orderings | spread |
|---|---:|---:|
| ∞ (Curless & Levoy) | **1** | 0.00000 |
| 4.0 | 36 | 0.08080 |
| 3.0 | **246** | 0.18516 |

**If your engine caps voxel weight, you have already lost order-independence.** Separately, float
`atomicAdd` isn't associative — 1000 values over 200 shuffles gave **55 distinct f64 results, 1 in i32
fixed-point.**

### Where the lattice actually belongs

Not on the voxel field. For 100,000 brush ops with a radius-8 brush touching ~2,145 voxels each:

| Design | Memory |
|---|---:|
| Edit log as a grow-only set (join = union), voxel field derived | **4.80 MB** |
| Per-voxel G-Set of brush contributions | **1,715 MB** |

**357×.** One brush op is a single lattice element covering thousands of voxels; a per-voxel G-Set
re-materializes it once per voxel. There's also a middle option nobody mentions: a **PN-counter vector over
replicas** — `3·R·4` bytes/voxel, exact same estimator, genuinely idempotent, bounded by *player count* not
edit count (24× memory at R=8, but only on the dirty working set).

### The assumption that actually breaks

Not numerics. **Dig and deposit do not commute semantically.** SDF union `min(a,b)` is a genuine
semilattice; subtraction `max(a,−b)` is not, and the two don't commute with each other. Dig-then-fill ≠
fill-then-dig as a matter of game meaning. CALM says a non-monotone program cannot have a coordination-free
consistent implementation, and no state representation repairs that. A dig-only world gets everything free.
A mixed world can still *converge* without consensus via a deterministic total order `(lamport, client)` —
but convergence isn't intent preservation, and a player's dig can be silently erased by a concurrent fill
that sorts later. That's a game-design decision the algebra won't make for you.

**Cheapest experiment (2 hours):** take your existing brush function, generate 8 overlapping ops, apply all
40,320 orderings to one brick, count distinct results by hash. One ⟹ you're on the unclamped monoid and
parallel/GPU/retry is already safe. More than one ⟹ fix that before any CRDT work. Then switch `(S,W)` to
i32 fixed-point and re-run — if distinct-count drops to 1, you bought bit-exact cross-replica determinism
for zero memory.

---

## 3. Rotation-dependent vertex placement → invariant theory

**The lift:** *"a map from local geometric data to an output point must commute with the group action —
by construction, not by training or tolerance."* → classical invariant theory; moving frames (Fels & Olver,
`10.1023/a:1005878210297`); **Villar et al., "Scalars are universal"** (`10.48550/arxiv.2106.06610`).

### Verdict: TRANSFERS — this is the implementable one

**The crux I flagged turned out not to bite, and that's the key insight.** I worried that a canonicalization
at a symmetric configuration must pick one of several equally valid frames, breaking equivariance. True —
*if you canonicalize*. Don't. The constraint on an equivariant map is `f(x) ∈ Fix_H(output space)` where
`H = Stab(x)`. For **frame-valued** output `Fix_H(SO(3)) = ∅` whenever `H ≠ {I}` — hence the impossibility
results (Dym et al., `10.48550/arxiv.2402.16077`; Kaba & Ravanbakhsh, `10.48550/arXiv.2312.09016`). For
**point-valued** output in ℝ³ it is never empty:

- `H = {I}` → `Fix = ℝ³`
- `H = Cₙ` (an edge) → `Fix =` the rotation axis
- `H` polyhedral (a corner) → `Fix = {`cell-symmetry center`}`

A vertex is a point. The obstruction doesn't bite, and where it constrains it constrains *correctly* — at a
perfect corner the vertex is forced onto the 3-fold diagonal, which is where the corner is. Verified: for
3 orthogonal normals with C₃ stabilizer, `f(C₃·data) = C₃·f(data) = (1,1,1)` exactly, no tie-break.

Moving frames genuinely **do not** transfer — they require a *free* action, and freeness fails at every
symmetric configuration, which for axis-aligned CSG is the majority case.

### Why Dual Contouring actually pops

Not conditioning. **The hard SVD truncation at σ < 0.1 is a discontinuous branch.** Over 20,000 trials with
σ_min seeded at the threshold in f32, the rank branch disagreed after a rotation in **454 cases**, and when
it flipped, `‖f(Rx) − Rf(x)‖` had **median 2.13, max 9.10** — a several-cell-width vertex pop from an
infinitesimal rotation. DC's QR fix targets accurate *evaluation* of `E[x]`; it fixed the wrong numerical
problem for equivariance purposes. (`Â` isn't equivariant anyway — Cholesky-type factors don't conjugate,
and the Givens sequence zeroes entries in a fixed world-axis order.)

### The rule

Exact, for a 3-crossing cell — Cramer on the 3×3 system, never forming `AᵀA`:

```
c  = (p₁+p₂+p₃)/3          dᵢ = nᵢ·(pᵢ−c)
x  = c + [ d₁(n₂×n₃) + d₂(n₃×n₁) + d₃(n₁×n₂) ] / [ n₁·(n₂×n₃) ]
```

Production form — branch-free, handles all degeneracies, Tikhonov via adjugate:

```
M = Σ nᵢnᵢᵀ        g = Σ dᵢnᵢ        λ ≈ 0.01
x = c + adj(M + λI)·g / det(M + λI)
```

`λ = 0.01` reproduces DC's σ = 0.1 truncation *smoothly*. Equivariant because `RIRᵀ = I`.
**No eigendecomposition, no SVD, no iteration, no data-dependent branch** — ~90 flops, zero warp
divergence, which is the practical GPU win over DC.

Measured equivariance residual, f32, coordinates in [0,256], 4000 random cells:

| rule | median | p99 | max |
|---|---:|---:|---:|
| DC normal equations | 6.80e−05 | 2.48e−01 | **5.6e+02** |
| dual basis (Cramer) | 1.61e−05 | 7.23e−04 | 3.6e−01 |
| **Tikhonov adjugate** | **1.59e−05** | **1.81e−04** | **6.4e−04** |

The tail is what a user sees: a 564-unit worst-case displacement versus a bounded 6.4e−04.

### The honest kill

**The motivating use case is unachievable by any vertex rule.** A 2D dual contourer with an exactly
equivariant rule, rotating a sharp CSG corner against a fixed grid:

| θ | verts | Hausdorff vs rotated reference |
|---:|---:|---|
| 0° | 54 | 0.000 cells |
| 1° | 54 | 0.059 cells |
| 15° | 68 | 1.686 cells |
| 90° | 54 | 0.000 cells |

Zero at 0° and 90°, linear in θ between. **The grid's symmetry group is the octahedral group (24
rotations), not SO(3)** — rotating a brush against a fixed lattice changes *which edges cross and where*,
so `sample ∘ rotate ≠ rotate ∘ sample`. No vertex rule fixes this; it's upstream.

What equivariance does buy: bit-identical meshes under 90°/180° rotations and lattice translations (with
magnitude-sorted 3-term dot products — measured 4328/9600 failures unsorted, **0/9600 sorted**); no pops;
and a cheaper divergence-free kernel. The user complaint about "not the same result" is almost certainly
the *pop*, not the O(h) resampling drift — drift is smooth and reads as antialiasing.

**Cheapest experiment (half a day):** instrument the existing DC path, log `σ_min(Â)` and whether the
truncation branch fired, sweep a brush 0–90° in 0.1° steps, histogram per-frame branch-state changes.
Predicted: pops correlate 1:1 with branch flips, not with `bᵀb`. If they don't, stop — the problem is
elsewhere. Then swap in the 8-line Tikhonov rule and re-run.

---

## 4. What this says about the method

Three lifts, three different answers — which is the evidence that the audit step is doing real work rather
than rubber-stamping analogies. The negative result (sheaves) was the longest and most sophisticated
output of the three, and it was still a no.

Two failure modes observed:

- **The corpus is not the bottleneck; the lift is.** Sheaf theory returned dozens of real papers and none
  of it transferred. Invariant theory returned one load-bearing citation and it was enough.
- **Verify agent claims about the corpus in both directions.** I doubted three stems this run and was
  wrong about all three — Transvoxel (paper + dissertation) and Ericson are present, acquired between my
  earlier sweep and now.

**Two lifts not yet run**, both with named target structures:

- *"manifold or intersection-free, pick one"* (Dreams) → **embedding vs immersion of a 2-complex in ℝ³**;
  named obstruction theories (Whitney, Haefliger), and the decidability results on embedding 2-complexes.
- *"a hierarchical cut where each node decides locally with no sibling communication"* (Nanite's
  "same input ⇒ same output") → **monotone predicates on lattices; matroids/greedoids**. Formalizing the
  cut-validity condition as a matroid would give provable guarantees about which cuts are legal.
