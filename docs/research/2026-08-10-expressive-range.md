# Expressive range of the composition grammar — FVS-R-9, run

**Date:** 2026-08-10
**Run:** `cargo run -p emerge-core --example expressive_range`
**Criterion:** `docs/research/2026-08-09-composition-grammar-decisions.md` §4, committed before this ran.
Raw counts: `2026-08-10-expressive-range.bins.ron`.

---

## 0. Verdict

**The approach as configured is falsified.** Row 1 fires as hard as it can:

| Row | Reading | Verdict |
|---|---|---|
| 1 — median enclosure **< 0.15** | **0.000** | **FIRES** |
| 2 — enclosure > 0.95 with opening density < 0.5 | 0.000 / undefined | pass |
| 4a — `H / ln 36 < 0.25` | 0.000 | **FIRES (vacuously — see §4)** |
| 4b — max bin share > 50% | 0.0% | pass (vacuously) |
| 3 — **the gate**, > 20% non-convergence | **0.0%** | pass |

Row 3 not firing is what makes the rest readable: **no solve failed.** All 128 converged, and all 128
produced **zero enclosed regions**. The histogram is empty. Nothing reached the (enclosure, opening
density) plane at all.

`site_67`-sized region, 12 × 12 cells, 14 prototypes from the four authored Site tiles.

---

## 1. Two explanations that look identical, separated

*"Every solve scored enclosure 0"* has two readings, and they call for opposite fixes. Both were
checked rather than argued.

**The metric can see a room.** `the_kit_can_build_a_room_the_metric_calls_enclosed`
(`grammar.rs`) lays a 5 × 5 room by hand from the kit's own prototypes — four corners, four walls,
floor — and `range::measure` reports **enclosure 1.0, one region, opening density 0**. Swap one top
wall for the doorway tile and it reports **1.0 with opening density 1.0**; knock a wall out and it drops
to **0.0**. So a zero from a solve is a statement about the solver, not about the measurement.

**And that room is legal.** The same test walks every orthogonally adjacent pair in the hand-laid room
and asserts each is permitted by the learned `support`. It passes. So this is **not** *"the grammar
cannot express a room"* — the grammar permits one and the sampling never finds it. That distinction is
the whole finding.

---

## 2. The mechanism, measured

The cell census over the run, against what the weights asked for:

| Prototype | asked (weight) | won (cells) |
|---|---|---|
| **`Empty`** | **20.00%** | **37.58%** |
| `site/tile_floor` | 20.00% | 17.86% |
| `site/tile_wall_n` (4 turns) | 20.00% | 16.28% |
| `site/tile_corner_nw` (4 turns) | 20.00% | 14.38% |
| `site/tile_doorway_n` (4 turns) | 20.00% | 13.90% |

**`Empty` takes nearly double its share, and every authored tile comes in under.** The reason is
structural rather than a tuning error: `Empty` presents nothing and may sit beside anything — that is
deliberate, and `from_compositions` says why (*"a grammar that cannot say 'nothing goes here' cannot
leave a doorway"*). But being compatible with every neighbour means propagation never eliminates it,
while every wall turn is eliminated by some neighbour somewhere. `Empty` is the one prototype that
survives everywhere, so it wins everywhere.

The consequence is visible in a single solve (`cargo run … --example expressive_range -- 7`): wall
stubs, corners and doorways scattered across the field, **none of them joining up**. Every wall run
terminates into `Empty` on its first step, because `Empty` is always available and nothing in a local
adjacency rule prefers continuation.

That is exactly row 1's authored reading — *"the solver makes wall confetti, not rooms"* — arrived at
from the number rather than from the picture.

---

## 3. What the corpus says the fix class is

Sandhu, Chen & McCoy, *Enhancing Wave Function Collapse with Design-level Constraints*
(`10.1145/3337722.3337752`), already cited by the decisions doc, names both mechanisms this
measurement implicates:

> "we extend the local constraint reasoning by incorporating **constraints that can work over any
> distance** and non-spatial constraints. Next, we further manipulate the generative space by
> introducing **weight recalculation** and dependencies."

A closed boundary is a property over distance. Plain WFC has no term for it, and no amount of
per-seam adjacency supplies one — which is why the arrangement can be legal and still never occur.

**So "author the nine missing tile kinds" is not, on its own, the answer this measurement supports.**
FVS-R-5's census stands and those tiles are still worth having, but adding corridor and dead-end tiles
to an alphabet whose `Empty` already wins 37.58% of cells changes the vocabulary without touching the
mechanism. The measurement's own finding is about weight and distance, not about vocabulary size.

---

## 4. Two honest caveats about the criterion itself

Recorded rather than fixed, because amending a criterion after seeing its output is the thing §4
exists to forbid. Both are candidates for a **pre-registered** amendment before the next run.

**The stopping rule was satisfied vacuously.** It stopped at the first block — `TV = 0.0000`, n = 64
against n = 64 — because both histograms were *empty*, and the total-variation distance between two
empty histograms is zero by definition. That is agreement, not convergence. No larger run would have
changed anything, so the conclusion stands, but the run length is not evidence of stability and the
report now says so in as many words.

**Rows 4a and 4b disagree on an empty histogram, and both readings are meaningless.** With no samples,
normalised entropy reads 0 and *fires*; max-bin share reads 0 and *passes*. Neither is a statement
about concentration, because there is nothing to concentrate. **Row 1 is the finding here; 4a's firing
carries no information.** An empty histogram should be its own outcome in the criterion rather than an
input to two concentration statistics — that is the amendment to pre-register.

---

## 5. What follows

The standing decision was *"do not add authored weights before FVS-R-9 runs."* It has run, so the
question is open, and it now opens with evidence rather than by feel. Three candidates, in the order
the measurement supports them, all of which must be re-measured against **these same committed rows**:

1. **`Empty`'s weight and role.** It asks for 20% and takes 37.58%. Whether the fix is a lower weight,
   or a rule that stops it being universally compatible, is a design question — but it is the term with
   the largest measured gap between what was asked and what happened.
2. **A constraint over distance**, in Sandhu's sense — something that can express "a wall run continues
   until it closes". This is the mechanism the corpus points at and the one plain WFC lacks.
3. **The nine missing tile kinds** (FVS-R-5). Worth doing, and *not* load-bearing for this result.

**FVS-R-16 stays unscheduled and its trigger is unchanged.** A VLM authoring a scoring function over an
alphabet whose generator produces no rooms at all would be optimising the weight of confetti.

Nothing here is enforced. The rows are read once by a human; `range`'s metrics carry the tests, the
verdict carries none, and nothing stops the next run from ignoring it.

---

## 7. Addendum — the weight hypothesis is falsified (`empty_weight`, same day)

§2 named `Empty`'s weight as the mechanism: it asks for 20% and takes 37.58%, so scaling it down
should let authored tiles win cells and let boundaries close. **That is wrong, and the sweep is
unambiguous.** `cargo run -p emerge-core --example empty_weight`, 128 solves per row, read against the
same pre-registered rows:

| w(empty) | empty % of cells | median enclosure | solves with any enclosed region |
|---|---|---|---|
| 1.00 (shipped) | 37.6% | 0.000 | **0** / 128 |
| 0.75 | 33.1% | 0.000 | 0 |
| 0.50 | 27.6% | 0.000 | 0 |
| 0.25 | 18.7% | 0.000 | 0 |
| 0.10 | 9.7% | 0.000 | 1 |
| 0.05 | 5.6% | 0.000 | 1 |
| **0.00** | **0.0%** | **0.000** | **2** |

**Deleting `Empty` from the output entirely buys two enclosed regions in 128 solves.** The lever works
— its cell share tracks the weight exactly, 37.6% → 0.0% — and the thing it was supposed to cause does
not happen. Median enclosure never leaves 0.000 at any setting.

Two further readings, both from the same table:

- **The alphabet is not over-constrained.** Row 3 never fires; every solve converges even at `w = 0`,
  where every cell must be an authored tile. So this is not the *"add tiles before judging the
  approach"* case either.
- **§2's diagnosis was a correlation.** `Empty` really is over-represented, and that really is
  explained by being compatible with everything — but over-representation was not what stopped rooms
  forming. Removing the correlate leaves the effect.

### What this leaves standing

The mechanism is **the adjacency relation, not the sampling distribution.** A closed boundary is a
property *over distance*: a wall run has to know it must eventually meet a corner that meets a wall
that returns. Local pairwise support cannot express that, and no reweighting of local choices creates
it — which is exactly what the sweep measures.

That is the term Sandhu, Chen & McCoy name and §3 already quoted, now with evidence behind it rather
than analogy:

> we extend the local constraint reasoning by incorporating **constraints that can work over any
> distance** and non-spatial constraints.

Their second term — *"weight recalculation"* — is the one this addendum rules out for this failure.
Both were listed in §3 as candidates; the sweep separates them.

**This also settles the FVS-R-5 question in the other direction.** §3 argued the nine missing tile
kinds were not the blocker because the room is *legal* under the learned support. The sweep
strengthens it: the blocker is not vocabulary and not weight, so it is not something an authoring pass
can fix. It is the solver's expressiveness.
