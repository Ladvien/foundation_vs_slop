# Composition-grammar decisions

**Date:** 2026-08-09
**Status:** decisions and invariants only — not a plan. Later steps are deliberately unspecified
because #25's output should determine them.
**Supersedes:** the four-move recommendation of the same day (see §5 for what was withdrawn).
**Companion:** `2026-08-09-grid-composition-corpus-check.md`.

---

## 1. Order

```
#25  convert site_67 architecture to stamps (with the floor-replacement caveat)
 ->  validity-function seam
 ->  move 1: grammar from compositions, including fixed-tile authoring
 ->  expressive range
 ->  #23: tags vs variants
```

#25 first, and **the original reason for that has been tested against the map and did not hold.**

It said: *"#25 first because it is what produces more than four prototypes and real co-occurrence
data. A grammar learned from a single harvested room is degenerate in one of two directions (§3), and
the fix is more examples, which is what #25 yields."*

**It does not yield that.** Converted (2026-08-10), `site_67` is 139 `site/tile_floor` + 5
`site/tile_wall_n`. The observable adjacency relation is **three pairs** — floor–floor,
floor–wall_n, wall_n–wall_n. That is not "more examples"; it is the same single room in a new
representation, with a *smaller* vocabulary than the placements it replaced. The map was a 12 × 12
slab with one straight wall on its west edge and nothing else, which nobody had counted before
converting it.

So the ordering stands and the justification is replaced. **#25 goes first for the representation
change and for the tile-gap census** — the thing that says nine tile kinds are missing, which is what
actually sizes the work after it. It does **not** go first for the training data, and no later step
should be scoped as though it did.

The consequence for move 1 is §3's LGG failure mode, now concrete rather than hypothetical: a learner
over three pairs can reproduce `site_67` and floor fields and nothing else. Karth & Smith's
*"might not allow any new output to be constructed that was not an exact copy of the source image"*
describes this input exactly, and the PCGML survey's *Learning from Small Datasets* is the general
condition — *"games are likely to always be data-constrained"* (`10.1109/tg.2018.2846639`).

**A second map is deliberately not in scope here** (author's call, 2026-08-10). It needs the corridor,
dead-end and closet tiles authored first, which trips `tests/site_tiles.rs`'s `comps.len() == 4` and
partly pre-empts #23. That ordering belongs to #23, not to #25.

---

## 2. Invariant — the validity-function seam

**`grammar::learn` must emit adjacency through a substitutable validity function, not by comparing
`Interface::faces` signatures directly.**

This is a constraint on how the commit is written, not an item to be completed. It exists because
promoting `Interface::faces` to the solver's contract *is* the colored-edges choice, and edge-vs-corner
was flagged as expensive to revisit before the schema is fixed.

Grounding — Karth & Smith (2019), `10.1145_3337722.3341845`:

> "The output of the `agrees()` validity function in Gumin's implementation just checks if two patterns
> can legally overlap, but **any arbitrary adjacency validity function can be substituted here.** As
> long as the validity function can be computed over all pairs of patterns, it can act as the whitelist
> for the constraint domains **without changing the WFC solver itself.**"

Consequences:

- Edge-matching is one implementation; corner-matching is another. Swapping is one function.
- The solver does not change. Move 1 is smaller than it was originally scoped.
- The corner decision (pending Lagae & Dutré — see the companion doc §8.4 for the retrieval route)
  stops being a prerequisite for anything.

---

## 3. Correction — harvesting gives prototypes, not a grammar

Box-select yields a *prototype*. It does not yield an *adjacency relation*. Capture-once is not the
feature; capture → solve → reject → capture again is, and that is not built.

Karth & Smith, same paper, on why one example is not enough:

> "A **LGG** learning strategy would say to only allow those adjacencies explicitly demonstrated in the
> source image. However, this highly-constrained alternative **might not allow any new output to be
> constructed that was not an exact copy of the source image.** Likely, the ideal amount of
> generalization falls somewhere between these extremes."

and on the remedy:

> "By allowing for multiple positive and negative examples and using a slightly altered learning
> strategy, we show how this meticulous work can be replaced with **a conversation that elaborates on
> past examples.**"

Production precedent for deriving adjacency from geometry rather than from example placements, same
paper: *"Bad North automatically detects alignment between the 3D geometry of neighboring tiles."*

---

## 4. Invariant — falsification criterion, chosen before the first solve

**Committed 2026-08-10.** A threshold chosen after seeing the output is not a threshold, and this is
the one item on this page that cannot be reconstructed later.

What is committed here is the **method, the metric definitions, three numeric rows, and the rule that
will set the fourth row's number.** The fourth number itself is deliberately *not* committed yet —
§4.4 says why, and that deferral is itself a commitment, made before any solve.

Grounding — `pcgbook-ch12-evaluating-content-generators`:

> "A tempting way to evaluate the quality of a content generator is to simply view the content it
> creates and evaluate the artifacts subjectively and informally... **If you see five levels that are
> impressive, among 50 that you choose to ignore or re-generate, what does that say about the qualities
> of the content generator?**"

### 4.1 The metrics

ch12's own metrics (linearity, leniency) are platformer-specific and do not transfer.

- **enclosure** — the fraction of floor cells lying inside a closed wall boundary, computed by
  flood-filling from the region's border across cells not separated by a wall face; cells the fill does
  not reach are enclosed. Range 0–1. Separates "makes rooms" from "field of disconnected wall segments."
- **opening density** — doorway tiles per enclosed region, as a mean over regions. Zero means sealed
  boxes; unbounded means walls with no rooms.

**Enclosure is at risk on ch12's own test, and that is recorded rather than resolved.** ch12: *"strive
to choose metrics that are **as far as possible from the input parameters** to the system… Choosing a
metric that is highly correlated to one that is used as an input parameter… **can only ever provide
confirmatory results.**"* Under a four-tile alphabet of `{tile_floor, tile_wall_n, tile_corner_nw,
tile_doorway_n}`, what fraction of cells close a boundary is very nearly a restatement of how many wall
and corner tiles the solver was permitted to place. **Opening density is the safer of the two** —
doorways per enclosed region is emergent from where the solver chose to puncture, not from the
alphabet's composition. This cannot be settled before the alphabet is fixed (§7's tile-gap census says
nine kinds are missing), so it is flagged here and revisited when it is.

### 4.2 The three rows that are committed outright

**The approach fails if, over a run sized by §4.4's rule:**

| Signature | Reading |
|---|---|
| median enclosure **< 0.15** | the solver makes wall confetti, not rooms |
| median enclosure **> 0.95** with opening density **< 0.5** | it makes sealed boxes nobody can enter |
| **> 20%** of solves return the `no arrangement satisfies what you have pinned` error | the alphabet is over-constrained; add tiles before judging the approach |

The last row is what §6's closed question buys: a failed solve is *named and loud*
(`grammar::solve` → `collapse_grid(..).ok_or_else(..)`), so it is countable rather than mistaken for a
mediocre success. That is the precondition — the heatmap is unreadable unless non-convergence is
distinguishable from convergence, because a sparse fill from a failed solve looks like a boring
successful one along every axis here.

### 4.3 The fourth row is a **concentration** statistic, not an occupancy one

The degenerate outcomes a small alphabet actually produces — uniform tiling and checkerboard — both
appear as **a single hot spot**. `pcgbook-ch12` reads its own Figure 12.1 that way: *"there is **one
large hot-spot** for creating medium-leniency, low-linearity levels… Understanding that the system is
biased towards these areas forces the designer of the system to ask why such biases exist."*

**Two formulations were written and rejected before this one. Both are recorded, because each failed
in a way that is easy to repeat.**

**Rejected 1 — "the 2-D histogram occupies < 5% of its populated bounding box."** A bounding box
computed *from the populated cells* is defined by them, so this is a fill ratio inside a box the data
drew. With every solve in one bin the box **is** that bin and occupancy is 100% — the statistic's
maximum. It is not that the row is backwards: it does fire on something real, a few outliers
stretching the box while mass stays central, which is "wide range, thinly explored". The precise fault
is that **the statistic is insensitive by construction to the case the row exists to catch, and
unrelated to it otherwise.**

**Rejected 2 — "fewer than 5% of bins occupied over 200 solves."** Occupancy is blind to
concentration. 180 solves in one bin plus 20 scattered singly across 20 other bins occupies 21 bins and
**passes**, with 90% of the mass in one hot spot — the named failure signature passing the criterion
written to catch it. It is also not sample-size invariant: bins-occupied is monotone in *n*, so
doubling the run makes the same generator pass more easily.

**Committed form — max-bin share.** *The approach fails if any single bin holds more than **X%** of
solves.* Dimensionless, sample-size invariant, and the direct numeric reading of "one hot spot".
Normalised Shannon entropy `H / log K` is the acceptable alternative and degrades more gracefully at a
bin boundary; occupancy may return as a *second* row, since it does catch "narrow range", but it may
not be the row that catches a hot spot.

**The better measurement exists in the corpus and is unavailable to this solver — recorded so it is
reconsidered if that changes.** Cooper (2022), *Sturgeon* (`10.1609/aiide.v18i1.21944`, the paper §5
already cites for tags), has a section titled **Expressive Range Coverage**: *"the proposed system can
**constrain the allowable range of that property and then generate a level**… with 6 ranges per
dimension… **Of the 36 possible levels, 19 were found.** … **7 timed out.**"* Constraining to each bin
and asking whether the solver can reach it is sample-size invariant, costs 36 targeted solves instead
of hundreds, and — decisively — reports *not found* and *timed out* as **different outcomes**, which is
exactly the precondition §4.2 needs.

It is not reachable here. `wfc::collapse_grid` (`crates/emerge-core/src/wfc.rs:250-257`) takes
`initial: &[u32]`, a **per-cell domain mask**, and its own comment names what that is: *"a narrowed
`initial` is a unary constraint."* `grammar::solve` uses it for one thing, pinning owned cells to a
single prototype. Enclosure is a **global** property of a finished grid, not a per-cell domain
restriction, so this solver cannot be asked for a solve *inside* a bin — Cooper's is clingo, which can.
Reject-sampling into bins is sampling with extra steps and is strictly worse than sampling.
**If the solver is ever swapped, this row is the first thing to revisit.**

### 4.4 The calibration rule — committed now, executed after the tile-gap census

**Do not commit a bin count, a solve count, or the value of X before the alphabet is counted.** The
achievable region of the (enclosure, opening density) plane is a function of the tile alphabet, and §7's
census finds nine missing kinds. A concentration floor picked now would fire on **alphabet poverty**
rather than on generator bias, and nothing in the measurement would distinguish them.

So the number is **derived by a rule fixed in advance**, which is the property this section exists to
protect. An uncalibrated number is not falsifiable; it is only unchangeable.

**The rule.** From `pcgbook-ch12`: *"generate **increasingly large amounts of content and visualize the
expressive range, stopping when the graphs begin to look the same** as for the previous, smaller amount
of content."*

1. Fix the domain grid before looking at anything: enclosure ∈ `[0, 1]`, opening density ∈ `[0, 4]`.
   Start at **6 ranges per dimension, 36 bins** — the granularity the corpus's one worked example of
   this exact measurement used, and unlike 400 bins it is not dominated by sampling noise.
2. Run at doubling sample sizes until the histogram stabilises between successive doublings. That, not
   a number chosen today, sets the run length.
3. Set **X** to the max-bin share of a *uniform* distribution over the bins the alphabet can actually
   reach, times a stated multiple. Both halves — the reachable-bin count and the multiple — get written
   down at calibration time, before the first heatmap is read for quality.

Committed as `2b` in the execution plan: **after the tile-gap census, before the first solve is judged.**

---

## 5. Withdrawn from the four-move recommendation

| Claim | Status |
|---|---|
| "Harvest by capture — already shipped, cost none, risk none" | **Wrong.** See §3. The adjacency half is not built. |
| Move 4, "leave dressing ungridded" | **Padding.** It is the status quo and a "don't do this," not a move. Conclusion stands; it isn't work. |
| Move 3 presented as a recommendation *and* deferred to #23 | **Incoherent.** Deferring is correct; it is a prediction about #23, not a move. |
| `Outcome::Partial` as a prerequisite for move 1 | **Dropped.** See §6 — verified not to be on move 1's path. |
| Move 1 before #25 | **Dropped.** Reordering was a slip, not a revision. Original ordering restored. |

Better argument for tags than the cross-product one, kept for #23: **under tags an absent mesh is a
missing renderer and the functional layer still solves; under variants the axis cannot be expressed at
all.** The Site kit having no wall-mounted piece is the evidence. This is what says do not wait on the
kit.

---

## 6. Verified: the panic is not on move 1's path

Checked against source this session:

- `wfc.rs:181` panics, but it is in `wfc::generate` — the dungeon substrate pass. The doc comment eight
  lines above says so: *"This is the substrate pass; the placement-grammar furniture pass degrades to
  `Outcome::Partial` instead."*
- `collapse_grid` returns `Option` — `None` on contradiction, for the caller to retry.
- The editor's **G** goes `grammar::learn` → that path, not through `generate`.

So the vetting doc's R3 is true of the substrate and does not gate move 1.

**Open, and a precondition for §4:** does `generate_from` surface a non-converged solve usefully rather
than quietly? A panic is the *good* failure — loud, and it tells you the alphabet over-constrained. A
quiet `None` → sparse fill reads as "the solver produced something mediocre," which is
indistinguishable from "the approach is mediocre," including in the expressive-range histogram.

---

## 7. Fixed tiles fold into move 1

The solver half exists. `generate_from` expands stamps into a scratch map specifically so the solver
sees them — *"the cells come back as the unary constraints they were always supposed to be."* Pinned
boundary cells as unary constraints is exactly the fixed-tile mechanism.

Missing: the authoring half — a way to say *"these cells are mine, fill the rest."* That is UI work,
not solver work.

Why inside move 1 rather than after it — Sandhu, Chen & McCoy (2019), `10.1145_3337722.3337752`,
measuring their constrained WFC variant:

> "the resulting conflict rate is around 60% for a map size of 100 tiles... it can be concluded that
> area propagation is best used as a constraint for **design time rather than runtime** due to the high
> conflict rate."

and their remedy: *"would be better for integration within a mixed-initiative tool such as Tanagra or
Sentient Sketchbook."*

Without pinned boundaries, the first real solve is the case they measured as failing. Fixed tiles are
also the thing that makes *"manual design and solved design aren't two workflows"* a feature rather
than a slogan.

---

## 8. Corpus hygiene

`10.1145_1814256.1814260` is catalogued as Smith & Whitehead, *Analyzing the Expressive Range of a
Level Generator*, but its indexed text is UCSC website boilerplate — the conversion captured a landing
page, not the PDF. **Do not quote it.** Cite `pcgbook-ch12-evaluating-content-generators` until it is
re-fetched and re-converted.

---

## 9. Housekeeping

`scripts/mirror_crates.sh` refuses while `crates/bevy_autogib/` is untracked; recent commits are
unmirrored. Correct behaviour — refusing beats forcing past a dirty tree — but it is an *unowned*
blocker: it clears only when the other agent commits. Worth a check rather than an assumption.
