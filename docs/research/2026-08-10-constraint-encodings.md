# Constraint encodings for the composition solver — cardinality, exactly-one, founded reachability

**Written 2026-08-10.** Answers question 2 of `docs/2026-08-10-constraint-solver-plan.md` §5. The sibling
docs answer *which solver* (`-solver-choice.md`) and *what the corpus holds* (`-pcg-solver-corpus.md`).

This is a specification, not a survey. Someone implementing `crates/emerge-core/src/constraints.rs` and
the L2 rule functions should not have to re-derive anything below. Where I am uncertain, §9 says so
rather than presenting a guess as settled.

**Sizing throughout:** the region `expressive_range` already measures — `site_67`'s slab, **12 × 12 =
144 cells, 14 prototypes** (`Empty`, `tile_floor` × 1 turn, and `tile_wall_n` / `tile_corner_nw` /
`tile_doorway_n` at 4 turns each). Every count in this document is computed for that instance and given
as a formula so a different region can be recomputed.

---

## 0. The one thing to take away

The plan calls founded reachability "the single most likely place to get it subtly wrong". It is, but
not where the plan expects. Two results dominate everything below:

1. **`seam_open(c, n)` factorises.** It reads as a conjunction over placement variables at two cells,
   which invites an encoding over all 14 × 14 = 196 prototype pairs per seam. That is wrong by two
   orders of magnitude. `range`'s wall predicate is a function of `(prototype, face)` alone, so the seam
   splits into two per-cell-per-face literals and the pair enumeration never happens. §1.2.

2. **The enclosure *lower* bound does not need foundedness at all.** Rules 1 and 2 alone are Horn, so
   every model has `outside ⊇ flood_fill`, so `¬outside[c]` is *sound evidence of genuine enclosure*.
   The rank machinery — 5,900 variables and 14,000 clauses — buys nothing for the constraint that FVS-R-9
   actually wants, and becomes load-bearing only when a wish pushes `outside` *true* (an enclosure
   ceiling, opening density, "the player can walk everywhere"). §5.2 proves this. §5.3 gives the rank
   encoding in full anyway, because §5.2's proof also says exactly when you will need it.

Everything else is a matter of picking encodings off a shelf and pinning the emission order.

---

## 1. Domain facts, read off the code

These are established from the source, not assumed. Every encoding below rests on them.

### 1.1 What the solver is choosing

`grammar::Grammar` (`crates/emerge-core/src/grammar.rs:79`) carries three things:

- `prototypes: Vec<Prototype>` — index 0 is always `Prototype::Empty`. `MAX_PROTOTYPES = 32`, because
  `wfc::collapse_grid` packs a domain into a `u32`. The SAT encoding has no such ceiling, but the
  ceiling stays until `collapse_grid` stops sharing the type (`grammar.rs:75`).
- `weights: Vec<f64>` — for a composition grammar, **one unit of weight per authored tile, split across
  the turns it survived dedup as** (`grammar.rs:1236-1242`). So the shipped kit's 14 prototypes carry
  weights `[1.0, 1.0, 0.25 × 4, 0.25 × 4, 0.25 × 4]`, summing to 5.
- `support: [Vec<u32>; 4]` — `support[dir][p]` is the bitmask of prototypes that may sit on `p`'s `dir`
  side. Directions are `wfc::{N, E, S, W}` = `0, 1, 2, 3`; `N` is `z − 1` and the grid is indexed
  `z * width + x` (`range.rs:142`, restating `wfc.rs`'s own convention rather than inventing a second).

`wfc::collapse_grid`'s per-cell `initial: &[u32]` domain mask is what the placement variables replace.
An owned cell is `initial[c] = 1 << ix` (`grammar.rs:283`); in SAT that is the unit clause
`place[c][ix]`, which is the same unary constraint said a different way.

### 1.2 The seam factorises — this is the load-bearing fact

`range::measure` blocks a seam like this (`range.rs:154`):

```rust
fn blocked(grid, here, there, dir, walls) -> bool {
    walls(grid[here], dir) || walls(grid[there], opposite(dir))
}
```

and `Faces::wall` (`range.rs:52`) answers from `self.faces[p].faces[dir]` alone — the wall predicate is a
pure function of `(prototype, face)`. There is no pairwise term anywhere. Therefore:

```
seam_open(c, n)  ⟺  ¬wall(tile(c), d)  ∧  ¬wall(tile(n), opposite(d))
                 ⟺  face_open[c][d]    ∧  face_open[n][opposite(d)]
```

where `face_open[c][d]` is **one Boolean per cell per face**, defined from the one-hot placement by 14
binary clauses:

```
place[c][p]  →   face_open[c][d]      for every p with ¬wall(p, d)
place[c][p]  →  ¬face_open[c][d]      for every p with  wall(p, d)
```

Every prototype falls in exactly one family, so the two together are 14 binary clauses per `(cell, face)`
and they define `face_open` in both directions given exactly-one-per-cell.

The naive alternative — a conjunction variable per allowed `(p, q)` pair per seam — costs 196 conjunction
variables and roughly 800 clauses per seam, times 264 seams: **51,744 auxiliary variables and ~200,000
clauses to express exactly the same Boolean function** that 528 variables and 7,392 binary clauses
express here. This is the part the prompt flags as the one people get wrong, and it is the whole of it.

Sturgeon makes the identical move one level coarser. Table 5 of Cooper 2022 (`10.1609/aiide.v18i1.21944`)
gives each *node* an `open` variable — *"a node is open iff its corresponding tile is an open tile"* —
encoded as `openTile → open` for each open tile plus `open → ⋁ openTiles`. Per-face rather than per-node
is the only difference, and it is forced on us because this kit's walls are one-sided
(`range.rs`'s `a_ring_of_one_sided_walls_seals_from_every_side` exists precisely because
`tile_wall_n` presents `wall` on a single face).

**Note the polarity difference between the two families.** Both are needed. The first forces `face_open`
*true* when the tile does not wall — that is what makes the flood fill reach as far as it really does
(§5.2's Claim A depends on it). The second forces it *false* when the tile does wall — that is what
stops the solver leaking the fill through a wall to make `outside` bigger than it should be. Dropping
either one silently changes which direction of the reachability constraint is sound.

### 1.3 The other two predicates

- `Faces::floor(p) = p != 0 && p < faces.len()` (`range.rs:78`) — **every prototype but `Empty` is
  floor**, including a solid wall tile. So `floor[c]` needs no variable at all: it is the literal
  `¬place[c][0]`. (This is why the hand-laid room in `the_kit_can_build_a_room_the_metric_calls_enclosed`
  scores enclosure 1.0 over all 25 cells including the wall ring — the ring is unreached floor.)
- `Faces::doorway(p)` (`range.rs:61`) is per-prototype too: it scans all four faces for a wall band whose
  lowest edge sits at or above `walkable`. One bit per prototype, precomputed.
- `Empty` presents nothing, so `bands()` returns `None`, so `wall(0, d)` is **false on all four faces**.
  Empty never blocks a seam. That is consistent with the fill and with `from_compositions`'s
  *"a grammar that cannot say 'nothing goes here' cannot leave a doorway"*.

### 1.4 The interface between the encoder and the metric

`enclosure_rules` takes `faces: &Faces` in the plan's L2 signature. **Derive the per-prototype tables by
calling `Faces::wall` / `floor` / `doorway` over `0..P` — do not re-read `Interface` directly.** The
constraint and the metric must read the same three predicates or they will drift, and a drift here
reproduces exactly the confusion FVS-R-9 spent a day separating: *the metric cannot see a room* versus
*the solver cannot make one*. One derivation:

```rust
struct ProtoFacts { walls: [bool; 4], floor: bool, doorway: bool }
// walls[d] = faces.wall(p, d); floor = faces.floor(p); doorway = faces.doorway(p)
```

### 1.5 Grid arithmetic, for the counts below

For `W × H` cells: `N = W·H` cells; undirected in-grid seams `S = W(H−1) + H(W−1)`; directed adjacencies
`2S`; border cells `2W + 2H − 4`. At `W = H = 12`: **N = 144, S = 264, directed = 528, border = 44,
interior = 100.**

---

## 2. `count(vs, lo, hi, weight)`

### 2.1 What the API has to serve

Four call sites, with wildly different shapes:

| Caller | n (literals) | lo | hi | hard/soft | how many |
|---|---|---|---|---|---|
| tile rules — exactly one prototype per cell | 14 | 1 | 1 | hard | 144 |
| distribution rules — per-prototype share | 144 | ~5 | ~15 | soft | 14 |
| enclosure — enclosed floor cells | 144 | `⌈want·N⌉` | N | soft | 1 |
| `min_rooms` — exactly one root per region index | 144 | 1 | 1 | hard | R (see §5.5) |

So `count` must be good at `n = 14, k = 1` and at `n = 144` with a `k` anywhere from 1 to 144. No single
encoding is best across that, which is fine — one function, one contract, and a **total, documented
decision rule** picking the construction (§2.5). That is a specialization inside one path, not two paths.
Sturgeon's own Table 1 footnote does the same: *"if lo is 1 in CNSTRCOUNT, a disjunction can be used"*.

### 2.2 The comparison

Sizes for "at most k of n", except where noted. "Preserves UP" means unit propagation on the encoding
enforces generalised arc consistency on the constraint — i.e. the solver derives every forced literal
without search.

| Encoding | Clauses | Aux vars | Preserves UP | Notes |
|---|---|---|---|---|
| **Pairwise / binomial** (AMO only) | `C(n,2)` | 0 | **yes**, in one step | `C(n,k+1)` for general k — explodes immediately |
| **Sequential counter / ladder** (Sinz 2005, CP) | `2nk + n − 3k − 1` | `nk` | **yes** (Sinz proves AC) | AMO case: `3n − 4` clauses, `n − 1` vars |
| **Totalizer** (Bailleux & Boufkhad 2003, CP) | `≈ n²/2` full; `O(n·k)` truncated | `≤ n⌈log n⌉` | **yes** | one tree serves *both* bounds; output is order-encoded |
| **Modulo / k-modulo totalizer** (Ogawa et al. 2013; PySAT's `kmtotalizer`) | `O(n^1.5)` / `O(n·k)` | fewer than totalizer | yes | Sturgeon's choice |
| **Sorting network** (Batcher; Eén & Sörensson 2006, JSAT) | `≈ 6·(n/4)·log n(log n+1)` | similar | **yes** | asymptotically `O(n log² n)` but the constant loses to a totalizer at n = 144 |
| **Commander** (Klieber & Kwon 2007) (AMO) | `≈ 3n` | `≈ n/2` | **yes** | good in the middle of the AMO range |
| **Bitwise / binary / log** (AMO) | `n⌈log n⌉` | `⌈log n⌉` | **yes** — see below | fewest clauses at small n; does not generalise past AMO |
| **Binary adder / PB** (Warners 1998; Eén & Sörensson) | `O(n)` | `O(n)` | **no** | cheapest to build, worst to solve — the solver re-derives arc consistency by search |

**On the bitwise encoding and unit propagation**, since this is commonly asserted both ways: it *does*
propagate. Worked, n = 4, codes `00, 01, 10, 11`, bits `b₁b₀`, clauses `xᵢ → (±b₁), xᵢ → (±b₀)`. Setting
`x₁` true unit-propagates `¬b₁, ¬b₀`; then `¬x₂ ∨ b₀` with `b₀` false yields `¬x₂`, and likewise `¬x₃`,
`¬x₄`. So AMO is arc-consistent under UP, in `⌈log n⌉ + 1` propagation steps rather than one. The real
objections are that it introduces variables the solver will branch on that have no meaning in the model,
and that it does not extend to at-most-k. Not that it fails to propagate.

**Concrete arithmetic at n = 144.** A full totalizer is `≈ n²/2 = 10,368` clauses. Batcher's odd-even
network padded to 256 inputs is `(256/4)·8·9 = 4,608` comparators × ~6 clauses ≈ **27,600** clauses. The
sequential counter for the *lower* bound (`Σx ≥ 5` becomes `Σ¬x ≤ 139`) is `2·144·139 + 144 − 417 − 1
≈ 40,000` clauses. **The totalizer is smallest here, and truncation makes it much smaller still.**

### 2.3 Recommendation

**Totalizer, truncated at `K = hi + 1`.** Four reasons, in order of weight:

1. **One tree serves both bounds.** `lo` and `hi` are each a single unit clause on the root's order-encoded
   output. The sequential counter needs a separate `O(n·k)` structure per direction, and the lower bound's
   `k` is `n − lo`, which is enormous for a small `lo`. This is decisive for the distribution rules, whose
   bounds are `±50%` of a target — a two-sided band on the same sum.
2. **A soft bound is one soft literal.** `weight: Some(w)` becomes a single label variable (§2.6). No
   part of the structure is duplicated or rebuilt. Sturgeon hits exactly this: *"since PySat only supported
   hard native atMostK constraints, all such soft constraints must be encoded"*, with label variables per
   Belov, Järvisalo & Marques-Silva 2013.
3. **Truncation is where the win is.** With `hi ≈ 15`, truncating each node's output at 16 cuts the
   distribution constraint from ~10,000 clauses to ~2,300.
4. **UP gives arc consistency**, so the solver prunes placement variables the moment a bound bites,
   instead of discovering it by backtracking.

I am recommending the plain truncated totalizer rather than PySAT's `kmtotalizer` (the modulo variant)
because at n ≤ 144 the modulo construction's `O(n^1.5)` saving is worth a few thousand clauses at the
cost of a materially more delicate implementation, and the mixed-radix carry logic is a second place to
get an off-by-one wrong. If a later region is 32 × 32 = 1,024 cells the trade flips; revisit then.

### 2.4 The totalizer, precisely enough to implement

Bailleux & Boufkhad 2003. Build a **balanced binary tree over the input literals, splitting each slice at
its midpoint** — the tree shape must be a pure function of `n`, because it fixes both the variable
numbering and the clause order (§6).

Each node `v` covers `m_v` inputs and owns output variables `s^v_1 .. s^v_{M_v}` where
`M_v = min(m_v, K)` and `K = min(hi + 1, n)`. The intended meaning is `s^v_i ⟺ (Σ inputs under v) ≥ i`.
A leaf owns no new variable: `s^leaf_1` *is* the input literal.

For an internal node `v` with children `a` (size `p`, outputs `M_a`) and `b` (size `q`, outputs `M_b`),
adopt the conventions `s^·_0 ≡ ⊤` (drop the literal from the clause) and `s^·_j ≡ ⊥` for `j > m_·` (drop
the literal from the clause). Then emit, for all `0 ≤ α ≤ min(p, K)` and `0 ≤ β ≤ min(q, K)`:

**Upward family** — forces the output high when the inputs are, and is what makes `≤ hi` work:

```
¬s^a_α  ∨  ¬s^b_β  ∨  s^v_{min(α+β, K)}          for α + β ≥ 1
```

**Downward family** — forces the output low when the inputs are, and is what makes `≥ lo` work:

```
s^a_{α+1}  ∨  s^b_{β+1}  ∨  ¬s^v_{α+β+1}         for α + β + 1 ≤ min(p+q, K)
```

Then the bounds themselves:

```
Σ vs ≥ lo   ⟹   unit clause    s^root_lo            (omit if lo == 0)
Σ vs ≤ hi   ⟹   unit clause   ¬s^root_{hi+1}        (omit if hi >= n)
```

**Emit only the family you need.** If `lo == 0`, the downward family is dead weight — skip it. If
`hi >= n`, skip the upward family. This halves the encoding for the enclosure constraint, which is
one-sided.

**Why the truncation is sound**, since this is the classic place to get it wrong. Under truncation the
semantics weakens to: `s^v_i ⟺ sum ≥ i` for `i < K`, and only `sum ≥ K → s^v_K` at the top index. We
assert `¬s^root_K`, and `sum ≥ K → s^root_K` gives `sum < K = hi + 1`, i.e. `sum ≤ hi`. Sound. The
downward family never references a truncated literal: it is emitted only for `σ + 1 = α + β + 1 ≤ K`, and
`α + 1 ≤ α + β + 1 ≤ K`, so the index `α + 1` is always within the child's retained range (or beyond the
child's true capacity `p`, in which case the literal is legitimately `⊥`). Both halves check out; write
that argument into the code as a comment, because a reader will otherwise "fix" the clamp.

**Clause count at a node:** `≈ 2·(M_a + 1)(M_b + 1)` when both families are emitted. Summed over a
balanced tree with no truncation this is `≈ n²/2`; with `K ≪ n` it is `≈ 2·n·K`.

**Refusals** — no fallbacks, per the crate's rules. `lo > hi` is a malformed call and must be a named
error. `lo > n` is unsatisfiable by construction and must be a named error at build time, not an
`UNSAT` discovered ten seconds later with nothing to say about why.

### 2.5 The encoding-selection function

`count`'s CNF must be a pure, documented function of `(n, lo, hi)`. This table *is* part of the spec,
because it determines the CNF and therefore the model:

```
lo > hi                        → Err("count: lo N above hi M")
lo > n                         → Err("count: lo N over M literals cannot be met")
lo == 0 && hi >= n             → emit nothing (vacuous)
lo == 0 && hi == 0             → n unit clauses  ¬xᵢ
lo == n && hi >= n             → n unit clauses   xᵢ
lo == 1 && hi >= n             → one clause  ⋁ xᵢ                        (at-least-one)
lo == 1 && hi == 1 && n <= AMO_PAIRWISE_MAX
                               → ⋁ xᵢ  +  C(n,2) pairwise  ¬xᵢ ∨ ¬xⱼ
lo == 1 && hi == 1             → ⋁ xᵢ  +  Sinz ladder at-most-one
otherwise                      → totalizer, truncated at K = min(hi+1, n)
```

`AMO_PAIRWISE_MAX` is the one number here that should be **measured rather than guessed**. §3 gives the
argument for a value around 16–32 and the arithmetic on both sides.

**Gotcha worth writing down:** because the selection is a function of `n`, authoring one more tile can
push a kit past `AMO_PAIRWISE_MAX` and change the CNF discontinuously, hence change the arrangement even
at the same seed. That is correct behaviour and it will still surprise someone. Say it in the doc comment.

### 2.6 Soft constraints and `unmet()`

`weight: Option<u32>`. Integer weights are not a stylistic choice — MaxSAT objectives are integers, and a
float weight would put a rounding path between the seed and the answer.

- **`None` (hard):** emit every clause of the encoding as a hard clause.
- **`Some(w)` (soft):** emit the *structure* hard, then introduce one fresh **label variable** `met`,
  add the hard clauses `met → s^root_lo` and `met → ¬s^root_{hi+1}` (i.e. `¬met ∨ s^root_lo`, `¬met ∨
  ¬s^root_{hi+1}`), and register `(met, w)` as a soft unit clause.

`Solution::unmet()` is then `Σ w over labels assigned false`. An optimising solver sets `met` true
whenever the bounds are jointly satisfiable, so `met` false means the wish was genuinely unmeetable —
which is exactly what turns today's flat refusal into *"everything except the north corridor"*.

**Why a label variable at all**, rather than just marking the clauses soft: a cardinality constraint
expands to thousands of clauses, and MaxSAT scores *clauses*, so soft-marking them all would price one
violated wish at thousands of units and make the objective meaningless. `implies_any(l, ms, Some(w))` is
a single clause and needs no label — emit `(¬l ∨ ⋁ms)` directly as a soft clause of weight `w`. State the
rule as: **a label is what you use when one logical constraint is many clauses.**

---

## 3. Exactly-one per cell

`count(place[c], 1, 1, None)` for every cell. At-least-one is the single clause `⋁_p place[c][p]`; the
question is at-most-one at `n = 14`, over 144 cells.

| Encoding | Clauses/cell | Aux vars/cell | Over 144 cells |
|---|---|---|---|
| Pairwise | 91 | 0 | 13,104 clauses, 0 vars |
| Bitwise (4 bits) | 56 | 4 | 8,064 clauses, 576 vars |
| Sinz ladder | 38 | 13 | 5,472 clauses, 1,872 vars |
| Commander (groups of 3) | ≈ 39 | ≈ 5 | ≈ 5,600 clauses, 720 vars |

**Recommendation: pairwise.** The spread is 5,500 to 13,100 clauses — a rounding error against the
~34,000 the whole problem needs (§7), so this decision must be made on properties, not size:

1. **Zero auxiliary variables.** Every variable in the placement block is a `place[c][p]` with a meaning,
   so `Solution::get` reads the grid directly and a dumped model is inspectable by a human. The ladder
   and commander encodings interleave 1,900–5,000 nameless variables into the block the debugger will be
   staring at.
2. **One-step propagation.** Pairwise derives every `¬place[c][q]` in a single UP round after a decision.
   The others take `O(log n)` or `O(n)` rounds. This matters more than it looks, because of §4.
3. **Fewest moving parts in the determinism story** (§6): no aux numbering to fix, no tree shape to pin.

**Where the recommendation flips.** At `MAX_PROTOTYPES = 32` pairwise is `C(32,2) = 496` clauses/cell =
71,424 over the grid, against the ladder's `3·32 − 4 = 92` clauses + 31 vars/cell = 13,248 clauses and
4,464 vars. That is where the quadratic starts to hurt. Setting `AMO_PAIRWISE_MAX = 16` keeps the shipped
14-prototype kit on pairwise and puts anything bigger on the ladder; setting it to 32 keeps every legal
grammar on pairwise. I would start at 16 and **measure** — this is one of the two numbers in this
document I am not willing to assert from first principles.

**The `min_rooms` root selection is the case that needs the ladder** (§5.5): exactly-one over 144 cells,
where pairwise is `C(144,2) = 10,296` clauses per region index against the ladder's 428 + 143 vars. Same
function, different `n`, different construction — which is why the selection rule in §2.5 is keyed on `n`
and not on the call site.

---

## 4. Pattern rules, and why they matter to this document

Not in the prompt's list, but the seam encoding and the plan's §3 checkpoint both depend on getting this
shape right, and it is two paragraphs.

For each directed adjacency `(c → n)` in direction `d`, and each prototype `q`, emit:

```
¬place[n][q]  ∨  ⋁_{p : q ∈ support[d][p]}  place[c][p]
```

That is: *if `q` sits at `n`, some prototype at `c` must support it*. **Unit propagation on this clause
family is exactly `wfc::propagate`.** `wfc.rs:327-334` computes `allowed = ⋃_{a ∈ domain(c)} support[dir][a]`
and intersects the neighbour's domain — which removes `q` from `n` when every `p` supporting `q` has been
eliminated at `c`, and that is precisely when the clause above becomes unit. Emitting it for both
orientations gives arc consistency in both directions, which is what the propagator's worklist achieves
by iterating to a fixed point.

This is the concrete statement behind the plan's §3 de-risking checkpoint. **L1 + tile rules + pattern
rules, solved for satisfiability with no objective, is arc-consistent WFC with backtracking added.** If
it does not reproduce the same *kind* of output — wall confetti, zero enclosed regions — the encoding is
wrong and no amount of reachability work will save it. And if it does reproduce it, that is also the
proof that the failure FVS-R-9 measured is not a WFC artifact: a complete solver over the identical
constraints still cannot make a room, because the constraints do not mention one.

Cost: `2S · P` clauses = 528 × 14 = **7,392**, each of width `1 + |support[d][p]|` ≤ 15.

---

## 5. Founded reachability

### 5.1 The rules, with the seam expanded

The plan's three rules, with `seam_open` replaced by the factorisation of §1.2 and with `border(c)` and
`opposite(d)` treated as the compile-time constants they are:

```
(R1)  border(c)                               →  outside[c]
(R2)  outside[n] ∧ seam[c,n]                  →  outside[c]
(R3)  outside[c]                              →  border(c) ∨ ⋁_n just[c][n]
      just[c][n]                              →  outside[n]
      just[c][n]                              →  seam[c,n]
      just[c][n]                              →  rank[n] < rank[c]
```

with the seam and face definitions:

```
(D1)  place[c][p]                             →   face_open[c][d]     ∀p: ¬wall(p, d)
(D2)  place[c][p]                             →  ¬face_open[c][d]     ∀p:  wall(p, d)
(D3)  face_open[c][d] ∧ face_open[n][d']      →  seam[c,n]
(D4)  seam[c,n]                               →  face_open[c][d]
(D5)  seam[c,n]                               →  face_open[n][d']
(D6)  enclosed[c]                             →  ¬place[c][0]     (floor)
(D7)  enclosed[c]                             →  ¬outside[c]
```

**`seam` is one variable per *undirected* seam** — `seam[c,n]` and `seam[n,c]` are the same variable.
`just` is one variable per *directed* adjacency, and is not symmetric. Enumerate seams by taking, for
each cell in row-major order, only the `E` and `S` directions where the neighbour is in-grid; that gives
each of the 264 seams exactly one identity, deterministically.

**`enclosed` needs only the two implications (D6), (D7), not the equivalence** — it appears only
positively, inside a `≥ lo` count, so nothing is lost by leaving the converse unstated. That is
Plaisted–Greenbaum polarity, and it halves the definition. The same reasoning does *not* apply to
`face_open` or `seam`, which appear in both polarities (see the note in §1.2), so those need both
directions.

### 5.2 Which direction actually needs foundedness

Write `O*` for the true flood fill — the least fixed point of R1 and R2 given a placement, which is
exactly what `range::reached_from_border` computes.

**Claim A (soundness of R1+R2 alone).** In every model of R1, R2, D1, D3, `outside ⊇ O*`.

*Proof.* Induction on the fill. Border cells: R1 forces `outside`. Step: if `n ∈ O*` and the real seam
`(c,n)` is open, then D1 forces `face_open` true on both faces and D3 forces `seam[c,n]`; the induction
hypothesis gives `outside[n]`; R2 then forces `outside[c]`. ∎

**Claim B (completeness).** For any placement, setting `outside := O*`, `face_open`/`seam` to their true
values, and `enclosed := floor ∧ ¬O*` satisfies R1, R2, D1–D7. So no arrangement is lost. ∎

**Corollary.** `¬outside[c]` in any model implies `c ∉ O*` implies `c` is genuinely enclosed. Therefore
**a lower bound on the count of `enclosed` literals is sound and complete under R1 + R2 alone.** R3, the
`just` variables, the ranks and the comparators are not needed. Adding R3 without ranks is *harmless* —
it removes some spurious models and leaves only sound ones — but it buys nothing.

**Where it flips.** The moment a wish pushes `outside` *true*, foundedness becomes load-bearing, because
R1+R2 are satisfied by `outside ≡ ⊤` for free. Concretely:

| Wish | Direction | Needs foundedness? |
|---|---|---|
| enclosure **≥** want ("make rooms") | pushes `outside` false | **no** |
| enclosure **≤** want (avoid §4.2 row 2's sealed boxes) | pushes `outside` true | **yes** |
| `min_rooms` ≥ R | can be done in the sound direction | **no** — §5.5 |
| opening density ≥ d per region | needs the region *exactly* | **yes** |
| "every floor cell reachable from a door" | pushes `outside` true | **yes** |

`Wishes` as drafted is `{ min_rooms, enclosure: f32, keep_distribution }`. **Whether `enclosure` is a
floor or a two-sided band is a design decision I am not making here.** A floor needs none of §5.3. A band
needs all of it. It is worth deciding before writing the code, because the difference is 5,900 variables.

### 5.3 The rank encoding, in full

Because §5.2 says you will need it as soon as the wish is two-sided, and because getting it right is
cheaper than getting it wrong.

**Bits per cell.** `B = ⌈log₂ N⌉` where `N = W·H`. At 144 cells, **B = 8** (domain 0..255, of which
0..143 are used).

The justification chain from any cell strictly decreases in rank, so it visits no cell twice, so it is a
*simple path* in the seam graph and has at most `N` cells — hence ranks `0..N−1` suffice. **Do not bound
`B` by the grid's geometric diameter (22 for a 12 × 12).** The seam graph is not the grid: a serpentine
corridor of open seams genuinely has a 144-cell path from the border, and a `B` sized for the diameter
silently refuses that legal arrangement. This is the failure I would bet on being made, because 5 bits
looks obviously sufficient for a 12 × 12 grid and is not.

**Pin the border to zero.** `rank[c] = 0` for every border cell — `B` unit clauses each, 44 × 8 = 352
units. This loses nothing (any valid rank function can be replaced by "hops along the justification
chain", which puts every border at 0) and it removes the enormous symmetry of shifting all ranks by a
constant. It also gives the right consequence for free: a non-border cell whose rank is 0 has no strictly
smaller neighbour, so it cannot be `outside`.

**The comparator.** Per directed pair `(c, n)`, `rank[n] < rank[c]` over `B`-bit unsigned integers, with
`r_n[k]`, `r_c[k]` the bit vectors (LSB = index 0). Introduce auxiliary literals `L_1 .. L_B` where `L_k`
means *"the low `k` bits of `rank[n]` are less than the low `k` bits of `rank[c]`"*.

`L` is used only **positively** (`just[c][n] → L_B`), so only the `L → definition` half is needed —
Plaisted–Greenbaum again. That halves it, and the missing half loses no solutions because the solver can
always set `L` true when the inequality genuinely holds.

Base case, `L_1 ⟺ ¬r_n[0] ∧ r_c[0]` — two binary clauses:

```
¬L_1 ∨ ¬r_n[0]
¬L_1 ∨  r_c[0]
```

Step, for `k = 1 .. B−1`, encoding
`L_{k+1} → (¬r_n[k] ∧ r_c[k]) ∨ ((r_n[k] ↔ r_c[k]) ∧ L_k)` — three clauses:

```
¬L_{k+1} ∨ ¬r_n[k] ∨  r_c[k]                 (bit k cannot be n:1, c:0)
¬L_{k+1} ∨  r_n[k] ∨  r_c[k] ∨ L_k           (both 0 ⟹ decided lower down)
¬L_{k+1} ∨ ¬r_n[k] ∨ ¬r_c[k] ∨ L_k           (both 1 ⟹ decided lower down)
```

Then `¬just[c][n] ∨ L_B`.

**Cost per directed pair:** `B` auxiliary variables and `2 + 3(B−1) = 23` clauses at `B = 8`. Over 528
pairs: **4,224 variables, 12,144 clauses**, plus 528 `just` variables and their 3 implications each
(1,584 clauses), plus R3's 100 clauses of width ≤ 5 (only the interior cells; the border cells' R3 is a
tautology and must be *omitted*, not emitted with a `⊤` literal), plus the 352 border rank units.

**Total for foundedness: 5,904 variables, 14,180 clauses.** That is the price §5.2 says you can decline
if the wish is one-sided.

### 5.4 The traps, named

1. **`B` sized to the geometric diameter.** §5.3. Silently refuses legal serpentines.
2. **Attaching the rank order to the static grid instead of to `just`.** Writing "adjacent outside cells
   have different ranks", or any rank monotonicity over the fixed 4-neighbourhood, is wrong in both
   directions at once: it constrains cells that are not `outside` (whose ranks must be free), and it
   fails to constrain the thing that matters, which is that the *chosen* justification decreases.
3. **Routing a chain through the border.** Prevented by omitting R3 for border cells *and* pinning their
   rank to 0. Do both; either alone is correct but the pair propagates far better.
4. **Emitting `just[c][n]` for off-grid neighbours.** The fill seeds every border cell unconditionally
   (`range.rs:172`) and never consults an outward face, so faces pointing off-grid have no `face_open`
   variable, no `seam`, and no `just`. An encoder that allocates all `4N` of each will produce
   unconstrained variables that a solver is free to set arbitrarily and a golden is free to be perturbed
   by.
5. **Forgetting D2.** Without *"a walling prototype forces `face_open` false"*, the solver can leak the
   fill through a wall to inflate `outside`. Harmless under a one-sided lower bound; a free win for the
   solver the moment the wish is two-sided.
6. **Assuming `just[c][n]` and `just[n][c]` need an explicit mutual exclusion.** They do not — the
   comparator already forbids both (`rank[n] < rank[c] < rank[n]`). Adding it is redundant clauses; it is
   worth *asserting* in a test, not encoding.

**What an unfounded loop looks like here**, so it can be recognised in a dump: two adjacent interior
cells `a`, `b` with an open seam between them and every other seam around them closed. R1 and R2 force
nothing. R3 is satisfied by `just[a][b]` and `just[b][a]`. The model says both cells are outdoors;
`range::measure` says they are a sealed two-cell room. **The symptom is the solver reporting a wish met
and the metric disagreeing** — which is the single most expensive class of bug available here, because it
looks like the metric is broken, and this project has already spent a day separating that exact pair of
explanations (`the_kit_can_build_a_room_the_metric_calls_enclosed` exists for that reason).

**The published exemplar exhibits this.** Sturgeon's Table 5 is a Clark completion of reachability plus
degree bounds (`CNSTRCOUNT(inEdges, 0, 1)`, `CNSTRCOUNT(outEdges, 0, 1)`, and
`MAKECONJ(¬startTile ⊕ ∀inEdge ¬inEdge) → noIn`, `noIn → ¬node`). Every node on a closed cycle has an
incoming reachable edge, so `noIn` is false and nothing forces it unreachable. Cooper reports precisely
that: *"in addition to the path from start to goal, the solver can include additional closed cycles off
the main path in the solution... shown in gold"*. Those cycles **are** the unfounded sets. Reading the
tabulated rules literally, I cannot see what would prevent the *goal itself* from being justified only by
a cycle with no connection to the start — but I have not read Sturgeon's implementation, only the paper,
so I state that as a question rather than a finding (§9).

### 5.5 `min_rooms` without foundedness

The plan's `Wishes::min_rooms` is a connected-component count, which sounds worse than reachability and
is in fact easier — because "these R cells are pairwise *un*connected" is a statement in the sound
direction, so Claim A carries it.

For each region index `i ∈ 0..R`:

- `root_i[c]` for every cell — `count(root_i, 1, 1, None)` (n = 144, so the ladder, §3).
- `reach_i[c]` for every cell, with a Horn closure over open seams restricted to floor:
  ```
  root_i[c]                                        →  reach_i[c]
  reach_i[n] ∧ seam[c,n] ∧ ¬place[c][0]            →  reach_i[c]
  ```
- Roots are enclosed floor: `root_i[c] → ¬outside[c]` and `root_i[c] → ¬place[c][0]`.
- Regions are distinct: `root_j[c] → ¬reach_i[c]` for every `c` and every ordered pair `i ≠ j`.
- **Symmetry breaking, or you pay `R!`:** require the roots' cell indices to be strictly increasing in
  `i`. Encode as `root_i[c] → ⋁_{c' > c} root_{i+1}[c']`, one clause per `(i, c)`.

`reach_i ⊇` the true reachable set by the same induction as Claim A, so `¬reach_i[r_j]` is sound evidence
that `r_i` and `r_j` are in different components. Each root is genuinely enclosed floor. Hence at least
`R` distinct enclosed regions, and no foundedness anywhere.

Cost at R = 3: 432 `root` + 432 `reach` vars; ~1,600 Horn clauses + ~1,300 exclusion clauses + 3 ladders.
Cheap. The one thing it does **not** give is an *upper* bound on regions, or the exact region for an
opening-density constraint — both of those are the founded direction.

### 5.6 What ASP gives instead

Nelson & Smith's chapter (PCG in Games ch. 8, in the corpus) writes reachability as three lines. Their
perfect-maze program, verbatim:

```prolog
linked(1,1).
linked(X,Y) :- parent(X,Y,DX,DY), linked(X+DX,Y+DY).
:- dim(X;Y), not linked(X,Y).
```

Ours would be:

```prolog
{ place(C,P) : proto(P) } = 1 :- cell(C).
face_open(C,D)     :- place(C,P), not wall_proto(P,D).
seam_open(C,N)     :- adj(C,N,D), opposite(D,D2), face_open(C,D), face_open(N,D2).
outside(C)         :- border(C).
outside(C)         :- outside(N), seam_open(C,N).
enclosed(C)        :- cell(C), not empty_at(C), not outside(C).
:- #count { C : enclosed(C) } < K.
```

**Under stable-model semantics `outside` is the least fixed point given the choice, which is the flood
fill by definition.** The justification rule R3 is never written; minimality supplies it. No `just`
variables, no ranks, no comparators — the 5,904 variables and 14,180 clauses of §5.3 collapse to two
rules. That is why Cooper used clingo and why the plan says ASP "gives this free".

What is actually happening underneath is worth stating for the solver-choice doc, because it is the
honest comparison rather than a slogan. Clark's completion (1978) is what R3 *is*; the completion of a
normal logic program is not equivalent to its stable models exactly by the unfounded loops of §5.4, and
Lin & Zhao (ASSAT, AIJ 2004) showed the gap is closed by adding a **loop formula** per loop. clasp's
conflict-driven answer set solving detects unfounded sets during search and adds the loop formula lazily,
so it pays only for the loops the search actually walks into. **The level mapping of §5.3 is the eager,
worst-case-priced version of the same thing** — you buy every loop formula up front whether or not the
search would have met it. Niemelä's and Janhunen's (in)translatability results are the formal statement
that you cannot have it both ways: there is no polynomial, faithful, *modular* translation from normal
logic programs to propositional theories, so either the encoding grows (level mappings, adding variables)
or the solver does the work at runtime (loop formulas, adding search).

The costs of taking clingo, stated flatly:

- **It is a C++ dependency.** `crates/emerge-core/tests/engine_free.rs` `ALLOWED_DEPS` is
  `serde, serde_json, ron, rand, rand_chacha, det_rng`. Widening that for clingo is a deliberate edit
  with an argument, or the solver lives in a new crate. That is the sibling doc's question.
- **Grounding.** Cooper measured the text front-end (`clingo-fe`) as *"quite slow"* and used clingo's
  backend API to bypass grounding. At 144 cells and 264 seams grounding is small, but the finding is that
  the front-end is not the fast path even at his sizes.
- **Determinism has the same shape as SAT's, not a better one.** clingo is deterministic for a fixed
  ground program, options and build, but the model can move on a version upgrade exactly as a SAT model
  can. Nothing about ASP makes §6 unnecessary.

### 5.7 The third option: solve, measure, cut

Worth naming because it is what clasp does internally and it is available without either dependency.
Solve with R1 + R2 only; run the real `range::measure` on the returned grid; if the model claimed an
`outside` the fill disagrees with, add a **blocking clause** over the placement literals responsible and
re-solve. That is lazy loop-formula generation by hand.

It is a legitimate algorithm rather than a fallback — one path, and the loop terminates because each
iteration removes at least one model. But the number of iterations is not bounded by anything useful, and
this repo's "no degraded result" rule means an iteration cap must produce a **named refusal**, not a
best-effort grid. That makes it a worse fit than it first appears, and it is only needed for the founded
direction, which §5.2 says may not arise. Listed for completeness; not recommended as the first build.

---

## 6. Determinism

The rule from the plan: the solver is deterministic given identical input, so **variety must come from
varying the problem per seed**, never from solver randomness. Concretely, that decomposes into three
separate obligations, and they have different failure modes.

### 6.1 The CNF must be a pure function of the input

Fix a canonical iteration order and use it for **both variable numbering and clause emission**. There is
exactly one order, used everywhere:

- cells: row-major, `z * W + x` — matching `grammar::solve` (`grammar.rs:298-300`) and `range::measure`
  (`range.rs:203`), because a third convention is a third chance to be off by a row.
- prototypes: grammar index order, `0..P`, with `Empty` at 0.
- directions: `wfc::{N, E, S, W}` = 0..4.
- undirected seams: for each cell row-major, directions `E` then `S`, in-grid only.

Variable blocks are allocated in this order:

```
1  place[c][p]            c row-major, p 0..P
2  face_open[c][d]        c row-major, d N..W, in-grid only
3  seam[e]                e in the E-then-S enumeration
4  outside[c]             c row-major
5  enclosed[c]            c row-major
6  just[c][d]             c row-major, d N..W, in-grid only            (founded only)
7  rank[c][b]             c row-major, b 0..B, LSB first               (founded only)
8  L[c][d][k]             c row-major, d N..W, k 1..B                  (founded only)
9  root_i / reach_i       i 0..R, then c row-major                     (min_rooms only)
10 totalizer node outputs post-order over the midpoint-split tree, per constraint, in declaration order
11 label variables        in soft-constraint declaration order
```

Clauses are emitted by the same nested loops, in the same block order.

Specific hazards in this code:

- **Never iterate a `HashMap`.** `grammar::learn` holds a `HashMap<(i64,i64), usize>` but only ever
  `contains_key`/`inserts` it (`grammar.rs:139`), which is why it is safe today. New code must use
  `BTreeMap` or a `Vec`, and the encoder should hold nothing hash-ordered at all.
- **`u32` support masks are already in a total order.** Walking a mask by
  `while m != 0 { let b = m.trailing_zeros(); m &= m - 1; }` — as `wfc::propagate` does
  (`wfc.rs:329-332`) — visits set bits ascending. No sort is needed to build a pattern-rule clause, and
  none should appear.
- **Aim for zero `sort*` / `min_by` / `max_by` calls in the encoder.** Every list is built from a range;
  there is nothing to sort. If one becomes unavoidable, see §6.4.

### 6.2 What the seed varies

The seed must reach the **problem**, not the search. Two mechanisms, and the second is the one I
recommend:

**(a) Jitter the soft bounds.** Draw the distribution rules' `lo`/`hi` around their targets per seed. This
is closest to Sturgeon's shape and it varies very little — a band of a few cells over 144 leaves most
optima identical.

**(b) A seeded per-cell preference (the "dream").** For each cell in row-major order, draw one prototype
from the grammar's own `weights` distribution and add the **soft unit clause `place[c][p_c]` with weight
1**. The optimum then trades "match this sampled arrangement" against the hard grammar and the enclosure
wish, and `unmet()` reports how much of the dream the constraints bent. 144 soft units, zero auxiliary
variables, and it replaces the distribution rules outright at a fraction of their cost (§7).

I recommend (b), and note it is *not* equivalent to Sturgeon's distribution rules: his constrain the
aggregate, this samples an instance whose aggregate matches in expectation. Both are defensible; the
choice is the caller's, and the arithmetic is in §7.

**Weight ladder.** With 144 dream clauses at weight 1, the dream's total mass is 144. A soft enclosure
wish at weight `W_enc` dominates the dream iff `W_enc > 144`. **Whether it should is a design decision
and I am not making it** — "enclosure is a wish that yields to the author's texture" and "enclosure is
nearly hard" are both coherent, and they produce visibly different generators. Whatever is chosen, write
the arithmetic next to the constant.

**Draw discipline.** Draw all 144 preferences up front, in row-major order, into a `Vec`, *then* emit.
Interleaving draws with emission couples the RNG stream to the clause-emission order, so a later
refactor that reorders emission silently moves every seed's output.

**Float or integer draws.** `det_rng::DetRng` gives `unit() -> f64` and `below(n) -> usize`
(`crates/det_rng/src/lib.rs`). Reusing `wfc::collapse_one`'s arithmetic — one `unit()` draw, walk the
weights subtracting — keeps the new path comparable to the old one at the §3 checkpoint. Quantising the
weights to integers and drawing with `below(total)` removes floating point from the seed→problem path
entirely, which is stronger. Rust does not contract to FMA or enable fast-math, so the f64 path is
IEEE-reproducible across platforms in practice and `GOLDEN_COLLAPSE_GRID` already depends on that; but
the integer path has nothing to argue about. I lean integer, and flag it as a decision because it makes
the checkpoint's comparison slightly less direct.

### 6.3 What the solver contributes, and why it must be pinned separately

Two artefacts need goldens, and conflating them is a mistake:

- **The CNF** is a pure function of `(map, grammar, faces, wishes, cell, seed)` and is entirely under our
  control. Pin it: a test that asserts `(var_count, clause_count, hash-of-clauses-in-emission-order)`.
  This catches an encoding reorder even when the reorder does not happen to change the optimum — which a
  model-level golden cannot. It is the direct analogue of `wfc.rs`'s `GOLDEN_COLLAPSE_GRID`, and it is the
  single highest-value test in this whole design.
- **The model** depends on the solver's build. **MaxSAT optima are frequently non-unique**, so the
  returned model is *one* optimum among possibly many, chosen by internal heuristics. Pin it too, but
  understand that a solver upgrade may legitimately move it while the CNF hash stays fixed — and that
  separation is what makes the upgrade auditable instead of mysterious.

This is a real change from today. `grammar::solve`'s `a_solve_is_reproducible_for_a_seed` currently holds
because the answer is a pure function of the seed and nothing else. Under MaxSAT it holds **per solver
build**. Say so in the test's doc comment rather than discovering it during an upgrade.

Two further solver-side requirements: **disable every randomisation knob** the chosen solver exposes
(random initial phase, random decision frequency, random seed), and **pin the version**. Core-guided
MaxSAT in particular extracts cores in an order that is sensitive to clause order when many soft clauses
share a weight — and with the dream, 144 of them share weight 1. That does not break determinism for a
fixed build; it does mean the model is unusually sensitive to §6.1, which is one more argument for the
CNF hash.

### 6.4 The `sort_total!` standard, and the gap

`sort_total!` (`src/util.rs:30`) and `tests/determinism_lint.rs` are the repo's answer to exactly this
class of hazard: every ordering site must declare whether its key is total, whether ties are
interchangeable, or why its input is not query-ordered. Two facts about applying it here:

1. **`sort_total!` is not reachable from `emerge-core`.** It is `#[macro_export]`ed by the game crate, and
   `emerge-core` is upstream of it and engine-free. So the available discipline in the encoder is
   `// SORT-OK: <reason>` — and, better, **not sorting at all**, which §6.1 says is achievable.
2. **The lint does not currently scan `crates/emerge-*`.** `tests/common/source_roots.rs` says so in as
   many words: *"`crates/emerge-*` are not here, and that is a known gap rather than an oversight...
   adding them is a measurement and a budget decision."* The measurement, run today:
   **27 ordering sites exist in `crates/emerge-core/src`** by the lint's own matcher, across
   `smart.rs`, `glb.rs`, `geom.rs`, `grammar.rs`, `clips.rs`, `import.rs`, `composition.rs`, `vocab.rs`,
   `adjacency.rs`, `stack.rs`. Some are inside `#[cfg(test)]` modules the lint exempts, so the real budget
   is smaller than 27 — but that is the ceiling, and it turns "a budget decision" into a number someone
   can decide with.

   Adding `"crates/emerge-core/src"` to `SCANNED_ROOTS` is the right end state, and it belongs in its own
   change with those numbers in the commit message, exactly as that module's doc asks. The encoder should
   be written to pass it from day one.

---

## 7. The budget

Everything, for the 12 × 12 / 14-prototype instance. Recommended design: pairwise exactly-one, pattern
rules both orientations, factorised seams, dream-based variety, enclosure as a one-sided soft lower bound.

| Block | Variables | Clauses | Note |
|---|---|---|---|
| `place[c][p]` | 2,016 | — | `N · P` |
| exactly-one per cell | 0 | 13,248 | `N · (1 + C(P,2))` |
| pattern rules | 0 | 7,392 | `2S · P`, width ≤ 15 |
| `face_open` + definition | 528 | 7,392 | `2S` vars, `2S · P` binary clauses |
| `seam` + definition (D3–D5) | 264 | 792 | `S` vars, 3 clauses each |
| `outside` + R1 + R2 | 144 | 572 | 44 units + `2S` ternary |
| `enclosed` + D6/D7 | 144 | 288 | polarity-halved |
| enclosure totalizer (lower bound only) | ≈ 750 | ≈ 5,000 | downward family only |
| dream soft units | 0 | 144 | one per cell, weight 1 |
| **subtotal, unfounded** | **≈ 3,850** | **≈ 34,800** | |
| R3 + `just` + comparators + rank | 5,904 | 14,180 | **only if the wish is two-sided** |
| `min_rooms` at R = 3 | 864 | ≈ 3,300 | optional |
| **total, everything on** | **≈ 10,600** | **≈ 52,300** | |

For scale: the *distribution rules* as literal cardinality constraints — 14 totalizers over 144 inputs
each, truncated at `K = 16` — cost roughly **10,500 variables and 139,000 clauses on their own**, four
times the entire rest of the problem. That is the single largest term in a literal transcription of
Sturgeon, and replacing it with 144 soft unit clauses is why §6.2 recommends the dream.

**None of these numbers are large.** 10,600 variables and 52,000 clauses is a small instance for any
modern CDCL solver; the difficulty here will be the *optimisation*, not the satisfiability. If solve time
becomes the problem, the first two things to try are lowering the enclosure weight (a cheaper optimum) and
tightening `AMO_PAIRWISE_MAX` — not shrinking the reachability encoding, which is 27% of the clauses at
worst and 0% under §5.2.

---

## 8. Build order

Matches the plan's §3 checkpoint and adds the two natural stops after it.

1. **`Problem` + `count` + `implies_any` + `conj`**, with the §2.5 decision rule and the §2.4 totalizer.
   Unit tests on the encodings alone: for small `n`, enumerate all `2ⁿ` assignments and assert the
   encoding's models project exactly onto the assignments satisfying the constraint. That is the one place
   where exhaustive testing is affordable and it catches every off-by-one in §2.4.
2. **Tile rules + pattern rules, satisfiability only, no objective.** The plan's checkpoint. Assert every
   adjacent pair in the returned grid is permitted by `support`, and that `expressive_range` reports the
   same *kind* of thing it reports now.
3. **Seam + `outside` + `enclosed` + a hard lower bound on enclosure.** No foundedness (§5.2). The oracle
   is `range::measure` on the returned grid: `enclosure ≥ want`, exactly. This is the first point at which
   the histogram can stop being empty.
4. **Soften it**, add the dream, and re-run the pre-registered rows.
5. **Only then**, if the wish turns out to need a ceiling or an opening-density term, add §5.3.

The mutation-test discipline from the plan's §6 applies to step 1's exhaustive tests especially: an
encoding test that passes with the downward totalizer family deleted is testing nothing.

---

## 9. Where I am uncertain

Stated rather than smoothed over.

- **`AMO_PAIRWISE_MAX`.** §3 argues 16 or 32 from clause counts. Which is better depends on how the
  solver's branching behaves with 4,500 extra ladder variables in the placement block, and I have no
  measurement. **Measure it; do not take 16 from this document as settled.**
- **PySAT's `kmtotalizer` attribution.** Sturgeon's footnote credits Morgado, Ignatiev & Marques-Silva
  2014 (which is the MSCG MaxSAT solver paper); PySAT's own documentation, as I recall it, credits Ogawa
  et al. 2013 for `mtotalizer` and Morgado et al. for `kmtotalizer`. I could not check PySAT's source from
  here. The recommendation in §2.3 does not depend on resolving this — I am recommending the plain
  truncated totalizer, whose construction §2.4 gives in full.
- **Sinz's exact clause count** (`2nk + n − 3k − 1`) is from memory of the CP 2005 paper. The asymptotic
  `O(nk)` and the arc-consistency result are solid; check the constant against the paper before quoting it
  in code comments.
- **Whether Sturgeon's implementation forbids a goal justified only by a cycle.** §5.4. The tabulated
  rules as printed appear not to, and Cooper reports the cycles as an observed artifact, but the paper is
  not the implementation and I have read only the paper.
- **Whether `Wishes::enclosure` is a floor or a band.** Not mine to decide, and it is the difference
  between §5.2 and §5.3 — 5,904 variables. Decide before writing the code.
- **Whether the dream replaces the distribution rules or sits alongside them.** §6.2. They are not the
  same constraint; the cost difference is 139,000 clauses; both are defensible.
- **Whether `opening_density` should be constrained at all.** It needs the founded direction and a
  region-indexed encoding, and it is currently a *diagnostic* in the pre-registered rows rather than a
  target. A cheap proxy — a soft global count of doorway prototypes — is available and is honestly a
  proxy, not the metric.

---

## References

- Bailleux, O., Boufkhad, Y. (2003). Efficient CNF encoding of Boolean cardinality constraints. *CP 2003*. — the totalizer of §2.4.
- Sinz, C. (2005). Towards an optimal CNF encoding of Boolean cardinality constraints. *CP 2005*. — the sequential counter / ladder.
- Eén, N., Sörensson, N. (2006). Translating pseudo-Boolean constraints into SAT. *JSAT* 2:1–26. — sorting networks, BDDs, adders.
- Ogawa, T., Liu, Y., Hasegawa, R., Koshimura, M., Fujita, H. (2013). Modulo based CNF encoding of cardinality constraints and its application to MaxSAT solvers. *ICTAI*.
- Klieber, W., Kwon, G. (2007). Efficient CNF encoding for selecting 1 from N objects. *CFV*.
- Warners, J. (1998). A linear-time transformation of linear inequalities into CNF. *IPL*.
- Belov, A., Järvisalo, M., Marques-Silva, J. (2013). — label variables for soft constraints; cited by Sturgeon's Table 1 footnote ‡.
- Clark, K. (1978). Negation as failure. — the completion that rule R3 is.
- Lin, F., Zhao, Y. (2004). ASSAT: computing answer sets of a logic program by SAT solvers. *AIJ* 157. — loop formulas; the gap between completion and stable models.
- Janhunen, T.; Niemelä, I. — (in)translatability of normal logic programs into propositional theories; why level mappings cost what they cost.
- Gebser, M., Kaufmann, B., Schaub, T. — conflict-driven answer set solving; clasp's lazy unfounded-set detection.
- Cooper, S. (2022). Sturgeon: tile-based procedural level generation via learned and designed constraints. *AIIDE*, `10.1609/aiide.v18i1.21944`. In the corpus.
- Nelson, M. J., Smith, A. M. (2016). ASP with applications to mazes and levels. *PCG in Games*, ch. 8. In the corpus — the three-line reachability recursion of §5.6.
- Aloul, F., Rawi, B., Aboelaze, M. (2006). Identifying the shortest path in large networks using Boolean satisfiability. — the SAT pathfinding Sturgeon's reachability rules are modelled on.
- Karth, I., Smith, A. M. (2017). WaveFunctionCollapse is constraint solving in the wild. *FDG*. — §4's identity between UP and the propagator is this claim made concrete.
- Sandhu, A., Chen, Z., McCoy, J. (2019). Enhancing Wave Function Collapse with design-level constraints. `10.1145/3337722.3337752`. — *"constraints that can work over any distance"*; the class this whole document is building.
