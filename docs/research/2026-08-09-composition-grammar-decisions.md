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

### 4.5 Executed 2026-08-10 — step 3 was unexecutable, and what replaced it

**Steps 1 and 2 stand. Step 3 is withdrawn.** It is recorded here rather than edited away, because it
failed in the same way §4.3's two rejects failed and that is now three times.

**Why it cannot be executed.** Step 3 wants the max-bin share of a uniform distribution over *"the bins
the alphabet can actually reach"* — and §4.3 has already established that this solver cannot be asked
whether it can reach a bin. `collapse_grid` takes a per-cell domain mask; enclosure is a global property
of a finished grid. So the only available estimator for the denominator is **the bins the run itself
occupied**, and `X = (1 / reachable_bins) × multiple` then reads its threshold off the same histogram it
judges:

| bins occupied | X at ×3 | largest possible max-bin share | fires? |
|---|---|---|---|
| 1 | 300% | 100% | **never** |
| 2 | 150% | 100% | **never** |
| 3 | 100% | < 100% | **never** |
| 4 | 75% | 100% | only above 75% |
| 9 | 33% | 100% | above 33% |

A generator putting **every solve in one bin passes.** That is precisely the single-hot-spot regime the
row exists to catch, and the rule then grows *stricter* the more diverse the generator is — wrong on
both ends. It also reintroduces **bins occupied**, the statistic §4.3 rejected as "Rejected 2", as the
term that sets the threshold, bringing back both of its faults (blind to concentration; monotone in *n*).

And it does the one thing §4.4 deferred the number to avoid: a poverty-limited alphabet occupies few
bins → small denominator → large X → **passes**. Counting the tile gap satisfied §4.4's precondition in
letter while leaving untouched the problem that precondition names. Knowing the alphabet is poor does
not tell you what X should be.

**A number computed from the output is not pre-registered, whatever the commit order.** The history
would show the *formula* preceded the run; it would not show that the *number* did.

#### The replacement — two rows, both functions of the fixed grid alone

Neither needs a reachable-bin term, so both were committed before the first solve. §4.3 named both
statistics rather than choosing between them, and the table below is why: each is blind where the other
sees.

**Row 4a — normalised Shannon entropy, `H / ln 36 ≥ 0.25`.** §4.3 already sanctioned this as *"the
acceptable alternative [which] degrades more gracefully at a bin boundary."* The floor is not a round
number. Because `K = 36 = 6²`, `ln 6 / ln 36 = ½` **exactly**, so the two-bin and three-bin uniform
values sum to ½ and are symmetric about ¼:

```
ln2/ln36 = 0.193428      0.250000 − 0.193428 = 0.056572
ln3/ln36 = 0.306572      0.306572 − 0.250000 = 0.056572
```

Equivalently, `H/ln K = 0.25` is an effective support of `36^0.25 = √6`, the **geometric mean of 2 and
3**. A floor equidistant between the two nearest canonical cases is the one that preserves the property
the statistic was chosen for; anything nearer either is settled by float precision and where bin edges
happen to land.

**Row 4b — max-bin share, `≤ 50%`: no single bin holds a majority of all solves.** §4.3 committed the
*form* — *"any single bin holds more than X% of solves"* — and deferred only X, and only because X had
been tied to a uniform-over-reachable baseline. A majority needs no baseline at all: dimensionless,
sample-size invariant, independent of K, derivable with zero run data.

**Both, because entropy is forgiving of a hot spot with a broad tail:**

| distribution | H/ln36 | max share | 4a @ 0.25 | 4b @ 50% |
|---|---|---|---|---|
| 90% one bin + 20 singles (§4.3's Rejected-2 case) | 0.174 | 0.90 | **FIRES** | **FIRES** |
| 2 bins, 50/50 | 0.193 | 0.50 | **FIRES** | passes |
| 3 bins, 80/10/10 | 0.178 | 0.80 | **FIRES** | **FIRES** |
| 70% one bin + 10 at 3% | 0.363 | 0.70 | passes | **FIRES** |
| 4 bins uniform | 0.387 | 0.25 | passes | passes |

70% of every solve in one bin is §4.3's named target walking through row 4a alone.

#### The rest of the method, pre-registered in the same commit

Each of these otherwise gets decided by whatever the code happens to do, after the fact.

- **Region:** 12 × 12 cells, matching `site_67`'s slab.
- **Doubling sweep over *disjoint* seed blocks.** Nested seeds (`1..n` ⊂ `1..2n`) make
  `TV(Hₙ, H₂ₙ) = ½ · TV(Hₙ, H′)` **exactly**, so a nested "TV ≤ 0.05" silently means 0.10 between
  independent samples. Compare block A (`1..n`) against block B (`n+1..2n`); stop at **TV ≤ 0.05**;
  report A ∪ B. Sizes 64, 128, 256, 512, 1024 (cap). The stopping rule sets the run length, not the cap
  — Smith & Whitehead run **10,000 levels** for every graph in the paper this method comes from, and a
  WFC solve over a 12 × 12 grid is far more expensive than a Launchpad level, so the cap is a budget and
  is reported as one if it is hit.
- **A region is a connected run of enclosed *floor* cells**, joined across seams no wall blocks — the
  interiors. Grouping every unreached cell instead would merge two sealed rooms into one through the
  wall ring they share, and *"doorway tiles per enclosed region"* would then count a corridor of rooms
  as one room. A doorway is counted against a region when it is orthogonally adjacent to it, whether or
  not that seam is open: it is the puncture in the wall, and the wall is what makes it a boundary.
- **Outside is not necessarily one place.** The fill seeds **every** border cell, not one corner — a
  wall running border to border splits the outdoors in two, and a single seed would report the far half
  as a room.
- **Zero enclosed regions** makes opening density undefined, and wall-confetti is the *expected* output
  given `Empty` at 36%. Such solves are **excluded from the opening-density median and from the
  histogram, and counted and reported as `no_enclosed_region`** — not mapped to 0, because 0 already
  means "enclosed regions with no doors" and conflating the two would make row 2 unreadable. The
  histogram's denominator is therefore solves with ≥ 1 enclosed region, and it is stated.
- **The `[0, 4]` clamp on opening density is reported separately.** An unbounded tail folded into the top
  bin inflates max-bin share by construction; row 4b must not fire on a binning artifact.
- **Failed solves are excluded and counted.** Note the **survivorship bias**: failure correlates with
  over-constrained configurations, so the surviving histogram tilts toward the easy region. §4.2's 20%
  row only partly covers it.
- **Row 3 is a gate, not a peer.** §4.2 says *"add tiles before judging the approach."* If more than 20%
  of solves fail, rows 1, 2, 4a and 4b are **not interpretable**, and the report must say so rather than
  printing co-equal verdicts.
- **§4.1's flag on enclosure carries forward unresolved.** The alphabet is still not fixed, so enclosure
  remains at risk of being *"highly correlated to an input parameter"* and therefore *"confirmatory."*
  Opening density stays the safer of the two.

### 4.6 Amendment — an empty histogram is its own outcome

**Pre-registered 2026-08-11, before the enclosure run.** FVS-R-18. The ordering is the whole point:
these two faults were found by the 2026-08-10 run and *recorded rather than fixed*, because amending a
criterion after seeing its output is exactly what §4 exists to forbid. They are fixed now, before the
next run, and **the four numeric rows in §4.2 and §4.5 are not touched.**

**The fault.** Both blind spots have the same root — an empty histogram was treated as a *value*
rather than as a distinct outcome.

1. *The stopping rule mistook blankness for convergence.* It stops when two disjoint blocks agree
   within `TV ≤ 0.05`, and the total variation between two **empty** histograms is 0 by definition. So
   the run stopped at its first block having measured nothing, and the report said "stable".
2. *Rows 4a and 4b disagreed on an empty histogram, and neither meant anything.* Normalised entropy
   read 0 and **fired**; max-bin share read 0 and **passed**. Both are statements about how mass is
   distributed, and there was no mass.

**The amendment, in three parts.**

- **`histogram_empty` is a reportable outcome**, listed beside `did_not_converge`, `no_enclosed_region`
  and `clamped`. When it holds, the run's finding is *"the generator did not reach the plane"* and that
  is the verdict — not a set of statistics about nothing.
- **Rows 4a and 4b are not evaluated on an empty histogram.** They report `n/a`, and `n/a` is not a
  pass. A concentration statistic over zero samples is undefined, and printing either verdict invites
  the reader to believe a measurement happened.
- **The stopping rule requires a non-empty histogram.** Two blocks agreeing counts as convergence only
  when at least one of them holds a sample. An all-empty sweep runs to the cap and is reported as
  having reached the cap without ever reaching the plane, which is the honest description.

**Rows 1, 2 and 3 are unchanged and still evaluated.** Row 1 reads the median enclosure over *solves*,
not over histogram bins, so it is well defined when nothing reached the plane — and it was the finding
in the 2026-08-10 run. Row 3 is a gate over solve outcomes and never touched the histogram at all.

**What this does not do.** It does not make an empty histogram a pass. The 2026-08-10 verdict stands
exactly as written: row 1 fired, the approach as configured was falsified, and nothing here revisits
that. This changes what the *next* run is allowed to claim, in the direction of claiming less.

---

### 4.7 Amendment — the evaluability floor, pre-registered 2026-08-17 (FVS-R-18)

Written independently of §4.6 on a parallel line and merged beside it 2026-08-17: §4.6 names the
empty histogram as its own outcome; this section adds the **floor** that keeps a nearly-empty one
from certifying anything either. Where the two overlap, they agree; the floor is the stronger rule
and the code implements both under §4.6's outcome wording.

FVS-R-9's run (`docs/research/2026-08-10-expressive-range.md`) exposed two blind spots in this
section, and §4's own discipline said they had to be **recorded then, amended before the next run** —
never patched while looking at the output they would judge. This is that amendment. It moves **no
committed threshold** (0.15, 0.95/0.5, 20%, 0.25, 50% all stand); it defines *evaluability*, which §4
never addressed because every rule above silently assumed the histogram would contain something.

**What the run exposed.** All 128 solves converged and all 128 produced zero enclosed regions, so the
histogram was empty — and two things followed that §4 did not anticipate. The stopping rule was
satisfied at the smallest block size, because total variation between two empty histograms is 0 **by
definition**: the sweep reported stability when what it had measured was that nothing reached the
plane at all. And rows 4a and 4b returned opposite vacuous verdicts on the same nothing — `H = 0`
fires 4a's `< 0.25` while a max-bin share of 0% passes 4b — a disagreement about no distribution.

**Rule 1 — the evaluability floor, `N_min = K = 36` in-histogram solves per block.** Derived from the
fixed grid alone (the uniform expectation of one solve per bin), so it is committable today without
touching any run's output — the same property every number in §4.5 was chosen for. A block whose
histogram holds fewer than `N_min` solves **cannot certify stability**: it does not enter the TV
comparison, whatever TV against it would read. A sweep that reaches the cap without two disjoint
blocks both at the floor agreeing at TV ≤ 0.05 makes **no stability claim** — it reports the cap as
the budget it is.

**Rule 2 — an empty histogram is its own terminal outcome, and rows 4a/4b are conditional.** A run
whose final histogram holds zero solves reports the outcome **`empty histogram`**, beside its
`no_enclosed_region` and failure counts — never "converged", "stable", or a row-by-row verdict. Below
the floor (including empty), rows 4a and 4b print `not evaluated — histogram below the evaluability
floor` and contribute nothing to the verdict, which retires the vacuous-fire/vacuous-pass
disagreement instead of adjudicating it. Rows 1 and 2 are unchanged: their medians are over per-solve
values, not the histogram, and row 1 firing on an all-zero run is the criterion working.

Encoded in `crates/emerge-core/examples/expressive_range.rs` in the same commit as this text, so the
next run cannot re-make the mistake by rerunning the old harness.

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

---

## 10. Decision — a lit wall is a **tag**, shaped as Cooper's second layer (FVS-R-6)

**Decided by the author, 2026-08-10.** This is the thing that gated the schema work, so it is written
down rather than left as the four unprompted arguments Step 4 kept making for it.

### The question was framed as a binary and the corpus does not answer it as one

`10.1609/aiide.v18i1.21944` — Cooper's *Sturgeon*, already cited in §4.3 for expressive-range coverage
— is the one system in the corpus that built this, and it separates the two things instead of choosing
between them:

> "a *tile* is simply an entity that can have **functional (i.e. gameplay) and/or image information**
> associated with it… Each level can consist of both a *functional grid* that defines gameplay for the
> level … and an *image grid* that defines what the level looks like."

> "It is possible to generate functional and image grids *simultaneously* by associating tiles with
> both … or *sequentially* by first generating a functional grid and then using that to limit image
> tile placement. **We found the sequential approach can be more efficient**, as reachability does not
> have to consider what a tile looks like."

And the link between the layers is exactly the mechanism this decision is about:

> "This work also uses *tags*, which are labels associated with one or more tiles and can be used to
> limit what tiles can be placed at a location. … **Sometimes the tile/tag distinction is intentionally
> blurred: functional tiles can be used as tags to constrain image tile placement.**"

A lit wall is *appearance over a functional wall*. Under Sturgeon's reading it is not a second tile and
not an inert label — it is the second layer, keyed to the first.

### What is committed

**Tags, not authored variants.** The argument that decided it is the one already on the record in §5,
and it is about what remains possible when a piece is missing rather than about the cross-product:
**under tags an absent mesh is a missing renderer and the functional layer still solves; under variants
the axis cannot be expressed at all.** The Site kit has no wall-mounted piece, so under variants the
sconce is not merely unauthored — it is unauthorable, and the decision would be blocked on an art task.

**And the tag field is shaped as Sturgeon's image grid**, not as a renderer hint: a layer keyed to the
functional tile, so *"a lit wall must face a room"* and *"never two adjacent"* are expressible as
constraints later. Plain tags — a label the solver never sees — make that a schema change rather than
an addition, and it is the same field either way, so the shape is free now and is not free later.

**Only the field ships. The second solve does not.** Writing a solver for a layer nothing yet
constrains would be building against a guess; `Composition` carries no tag axis at all today
(`crates/emerge-core/src/composition.rs:87` — `id`, `envelope`, `members`, `locations`, `note`), so
FVS-R-8 adds one field, and the sequential solve arrives when something asks for it. That is one path,
not a stub: a field with no reader is data the author writes and the renderer reads, which is what the
Descriptor's `kind`/`look` already are.

### What this settles for FVS-R-8

The occupancy test and the typed split values keep the size they had. The tag axis adds **one field on
`Composition`**, not a second prototype family — which is what the variant answer would have cost, and
what made this item gate the schema work in the first place.

### The falsifier, so this can be wrong later

If the appearance layer ever needs an adjacency relation the *functional* layer cannot supply — a rule
between two lit walls that does not reduce to a rule between the walls under them — then the layers are
not nested and Sturgeon's sequential form is the wrong shape. Nothing in the Site kit does this today.
That is the thing to check before writing the second solve, not after.
