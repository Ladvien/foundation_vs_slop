# Compose-carousel plan — what the corpus says

**Date:** 2026-08-10
**Question:** the five-item plan for finishing `compose-carousel` — does home-still support it, and does it
survive the literature it is already built on?
**Method:** `distill_search` over the local corpus (9 queries), `catalog_read` to verify two
corpus-hygiene claims. Companion docs read first:
`2026-08-09-composition-grammar-decisions.md`, `2026-08-09-grid-composition-corpus-check.md`,
`2026-07-05-placement-grammar-research-vetting.md`.
**Scope note:** the connected folder is `docs/research` only. Every claim below about
`compose.rs`, `input.rs`, `site_67.map.ron` etc. is taken from the plan's own text, not read from
source. What is vetted here is the *reasoning*, against the corpus.

---

## 0. Verdict

| Item | Corpus verdict |
|---|---|
| **1.** Revert `library.ron` | Outside the literature. No comment either way. |
| **2.** §4 falsification thresholds | **Diagnosis right, replacement wrong.** The new row still cannot catch a hot spot, is not sample-size invariant, and reinvents — in its weaker form — a construct the corpus already holds. |
| **3.** FVS-R-12, `Text` input | **Endorsed without qualification.** The physical/logical split and the whole-string call are exactly the raw-input → high-level-action move two testing papers advocate. |
| **4.** Seven review findings | **4b directly supported** by Infinigen's support relation, including the refusal. The rest are code hygiene the corpus does not speak to. |
| **5.** Convert `site_67` to stamps | **Two problems.** The 144-gesture pixel driver is the pattern two corpus papers name as brittle; and the item's own analysis falsifies the reason the decisions doc gave for scheduling it first — which the plan notices and does not say. |
| **Order** | One real dependency is inverted, and it is not the one the plan argues about. See §6. |

The highest-value single finding is **§5.3**: item 5's analysis kills the stated justification for item 5's
own position in the sequence, and the plan records that as a parenthetical.

---

## 1. Item 2 — the replacement threshold does not catch the thing it replaced

### 1.1 The diagnosis is right, and the wording overshoots

The plan's argument against

> the 2-D histogram occupies **< 5%** of its populated bounding box → one hot spot

is correct where it matters. With every solve in one bin, the populated bounding box *is* that bin,
occupancy is 100%, and a `< 5%` test cannot fire. Committing that reasoning is worth doing.

But "it fires on the opposite of what it names" overshoots. The statistic fires when a few outliers
stretch the box while mass stays central — "wide range, thinly explored," which is a real property, just
not the named one. The precise claim, and the one that survives being argued with, is: **the statistic is
insensitive by construction to the case the row exists to catch, and unrelated to it otherwise.** In a
document whose whole purpose is numbers that survive argument, that distinction should be in the text.

### 1.2 The replacement measures occupancy; the failure signature is concentration

The decisions doc names what the row is for:

> "The two degenerate outcomes a small alphabet actually produces — uniform tiling and checkerboard —
> both appear as **a single hot spot**. That is the failure signature to watch for."

The plan's replacement: *fails if fewer than 5% of bins (< 20 of 400) are occupied over 200 solves.*

Bin-occupancy count is blind to concentration. 180 solves in one bin plus 20 solves scattered singly
across 20 other bins occupies 21 bins and **passes** — while 90% of the mass sits in one hot spot. That
is the failure signature passing the criterion written to catch it.

Expressive range analysis reads heat maps for hot spots, not for fill. `pcgbook-ch12`, on Figure 12.1:

> "there is **one large hot-spot** for creating medium-leniency, low-linearity levels, and another bias
> towards creating medium-leniency, high-linearity levels... Understanding that the system is biased
> towards these areas forces the designer of the system to ask why such biases exist."

**Fix:** state the row on a concentration statistic, not an occupancy count. Two forms, either defensible:

- **Max-bin share** — fails if any single bin holds more than *X*% of solves. Dimensionless, fires
  immediately on the degenerate case, and is the direct numeric reading of "one hot spot."
- **Normalised Shannon entropy** of the bin distribution, `H / log K`. Also dimensionless, and degrades
  gracefully rather than tripping on a bin boundary.

Occupancy can stay as a *second* row — it does catch "narrow range" — but it cannot be the row that
catches a hot spot.

### 1.3 Occupancy is not sample-size invariant, and ch12 says how to pick *n*

Bins-occupied is monotone non-decreasing in sample count. "≥ 20 of 400 bins over 200 solves" is therefore
a joint claim about the generator *and* the run length: double the solves and the same generator passes
more easily. A threshold that moves with a number the plan picked arbitrarily is not the falsification
criterion §4 exists to be.

ch12 supplies the missing discipline:

> "One method to ensure an acceptable sample size in the case of infinite content is to generate
> **increasingly large amounts of content and visualize the expressive range, stopping when the graphs
> begin to look the same** as for the previous, smaller amount of content."

So commit the **rule** — run until the histogram stabilises between successive doublings — not the
number. A count chosen before the first solve, with no data on solve cost or variance, is a guess wearing
a threshold's clothes; it fails §4's own standard for a different reason than the one §4 was guarding
against.

Two arithmetic notes on the proposed grid. 200 samples across 400 bins is 0.5 samples per bin — any
occupancy statistic there is dominated by sampling noise. And the corpus's one worked example of exactly
this measurement used **6 ranges per dimension, 36 bins**, not 400.

### 1.4 The corpus already holds this construct, done better, in a paper the project already cites

Sturgeon — Cooper (2022), `10.1609/aiide.v18i1.21944`, the paper the decisions doc §5b already leans on
for tags — has a section titled **Expressive Range Coverage**:

> "For certain properties of a level, the proposed system can **constrain the allowable range of that
> property and then generate a level**; when done over a number of properties and ranges, this can
> demonstrate the expressive range coverage of the generator."

> "Using the mario_bros (ring) setup, we explored the coverage along two dimensions, using number of gap
> tiles and number of solid tiles, with 6 ranges per dimension... **Of the 36 possible levels, 19 were
> found.** Most of the levels not found required few or many solid tiles; **7 timed out.**"

Same idea, inverted: don't sample and count where you landed — constrain to each bin and ask whether the
solver can reach it. What that buys, point by point against the plan's version:

- **Sample-size invariant.** One targeted attempt per bin. §1.3 stops being a problem.
- **Separates *can't* from *didn't*.** Cooper reports *not found* and *timed out* as different outcomes.
  That is precisely the distinction the decisions doc §4 names as a precondition: *"the heatmap is
  unreadable unless non-convergence is distinguishable from convergence."* The sampled form conflates
  them; the constrained form reports them separately by construction.
- **Cheaper.** 36 targeted solves against 200 sampled ones.
- **Needs machinery item 5 already needs.** Pinning cells as unary constraints — decisions doc §7:
  *"`generate_from` expands stamps into a scratch map specifically so the solver sees them — the cells
  come back as the unary constraints they were always supposed to be."*

**Caveat, stated rather than buried.** Cooper's system is a full constraint solver (clingo) that can be
*asked* for a level satisfying a range on a metric. A greedy WFC collapse cannot necessarily
be constrained that way, and whether `generate_from` can accept a range constraint on enclosure is a
source question I did not check. So: **if it can, use Cooper's form and drop the sampled row entirely.
If it can't, keep the sampled form and fix it per §1.2 and §1.3.** Either way this belongs in §4 as the
argued choice, because right now §4 is about to commit the weaker option without knowing the stronger one
is in the library.

---

## 2. Item 2, continued — the metric pair is at risk on ch12's own test

ch12's rule of thumb for choosing expressive-range metrics:

> "strive to choose metrics that are **as far as possible from the input parameters** to the system...
> Choosing a metric that is highly correlated to one that is used as an input parameter... **can only ever
> provide confirmatory results.** If the system is specifically designed to create a particular kind of
> output, measuring for that output can only show that the algorithm operates as expected."

**Enclosure** — "proportion of cells whose walls form a closed boundary" — is close to an input under a
four-tile alphabet of `{tile_floor, tile_wall_n, tile_corner_nw, tile_doorway_n}`. What proportion of
cells close a boundary is very nearly a restatement of how many wall and corner tiles the solver was
permitted to place. **Opening density** is safer: doorways per enclosed region is an emergent property of
where the solver chose to puncture, not of the alphabet's composition.

This is not fatal — ch12 offers a rule of thumb, not a proof — but it is the kind of thing §4 should have
argued and hasn't. And it cannot be argued *before* the alphabet is fixed, which is §6.

---

## 3. Item 3 — endorsed, and it is the item the plan gets most right

`InputKind::Text` with `text: Option<String>` (one call per string, not one per character) and the
physical/logical `KeyCode`/`Key` split is, in the testing literature's vocabulary, the move from **raw
input replay** to a **high-level action**. `10.1016/j.entcom.2016.08.002` — Hernández-Bécares, Costero &
Gómez-Martín, *An approach to automated videogame beta testing*:

> "instead of injecting input events such as **'spacebar, left, left, spacebar, right, return'** we are
> going to reproduce the real actions recorded, which represent the result of replaying a list of
> keystrokes and mouse movements."

`queue_text("my_tile")` *is* that. The end-to-end acceptance — *an agent can name a composition end to
end* — is a high-level test in the same paper's sense, and is the right closing condition.

Two smaller alignments worth noting rather than changing:

- The same paper argues for recording and diffing the **internal message trace**, not just the pass/fail
  outcome: *"some discrepancies could have occurred at the messages level... a signal showing that
  something has changed. Maybe it is not enough to modify the outcome of the test, but sufficient to
  provoke internal changes."* Asserting on emitted `KeyboardInput` messages in `input_edges.rs` is already
  that idiom.
- "Mutation-test it" is good practice and only weakly corpus-backed. `10.48550/arXiv.1906.10742` (Zhang,
  Harman, Ma & Liu) names mutation testing as a standard test-adequacy criterion — *"Test adequacy
  evaluation aims to discover whether the existing tests have a good fault-revealing ability"* — and
  Marwedel's *Embedded System Design* covers fault injection generally. Neither is game- or Rust-specific.
  Flagging as a not-corpus-backed instruction, not as a criticism.

---

## 4. Item 4b — directly supported, refusal included

`compose.rs:515` computing `tallest = tallest.max(m.lift + top)` treats every member as floor-standing.
Infinigen Indoors, `10.48550/arxiv.2406.11824`, defines the relation the code is skipping:

> "**SupportedBy** specifies a relation using a child object's planar surface and a parent object's planar
> surface... the surfaces are parallel against each other with zero margins, and the centroid of the child
> object is contained within the convex hull of the intersection between the child and the parent object."

> "**StableAgainst** specifies a relation using a child object's planar surface, a parent object's planar
> surface, and a margin between the surfaces."

Two things follow, and the plan has both:

1. A member's height is defined *through* its host. Resolving host-first is a topological sort over the
   support graph, not an optimisation.
2. Infinigen's relations are *"a predicate that can be True or False of any pair of objects."* An absent
   parent makes the predicate unsatisfiable — there is no default. The plan's *"a member whose host is
   missing is a refusal naming the member, not a fall back to the floor"* is the same reading.

Tutenel et al. (`10.1609/aiide.v6i1.12398`) reaches it via typed features instead of relations; the
conclusion is identical, and `2026-07-24-placement-rule-unification.md` already builds on that paper.

**4a** (returning a centre alongside the size so `Anchored` and `Bounded` are commensurable) is standard
AABB centre-plus-half-extent hygiene. Correct, needs no citation. **4c–4g** are code-correctness findings
outside the corpus; I read the arguments and they are internally consistent, which is not a corpus
verdict and I am not claiming one.

---

## 5. Item 5 — the driver, and the finding the plan understates

### 5.1 The 144-gesture pixel driver is the pattern two corpus papers name as brittle

The plan: query the camera once, project 144 cell centres to viewport pixels locally, then per cell inject
`Cursor` + `Mouse` press/release, 2 frames per stamp.

Hernández-Bécares, Costero & Gómez-Martín, `10.1016/j.entcom.2016.08.002`:

> "**even the slightest modification of the map makes the raw input files invalid** and they have to be
> recreated by playing the level again"

> "The previous raw game replay is not suitable when the map level has been changed, since **using blind
> input injection will make the player wander incorrectly** in search of places and entities that may have
> been moved, modified or even removed from the game."

Aroudj & Ostrowski, `s40601-013-0010-4`, *Automated Regression Testing within Video Game Development*:

> "**A changed position of a user interface element (UIE) in the AUT renders the playback of corresponding
> test scripts invalid since recorded mouse clicks still use outdated position data.** The maintenance
> effort of such recordings is high and prone to playback errors."

The plan half-anticipates this — *"a driver that dies at cell 90 still leaves 90 stamps down"* — and
answers it with a copy-and-diff. But the failure mode these papers describe is worse than dying: one
camera nudge, one panel appearing, one scale change, and the projected coordinates are wrong **silently**,
because a click that lands on nothing is not an error. There is no assertion in item 5's acceptance that
would catch 12 stamps landing one cell off; the acceptance is a captured frame.

Note the interaction with **item 4c**, which fixes a `UiScale = 1.2` bug in a world-to-viewport projection.
Item 5 then drives clicks through a world-to-viewport projection. If any part of that path is
scale-sensitive, item 5 inherits the exact class of defect item 4 just removed, and the only check is a
screenshot.

### 5.2 The recorded finding is the wrong shape

The plan says: *"there is no flood-fill, box-fill or clone path that lays a composition stamp... Record
this as FVS-R-5's second finding."*

The literature reframes it. The missing thing is not a convenience verb — it is the **semantic action
layer**. FVS-R-14 ("a stamp as a selectable identity") is not out of scope for item 5; it is the thing
that makes item 5 mechanisable, and doing item 5 by coordinate replay pays the cost of its absence 144
times while filing it as a note.

Three options, in the order the corpus supports:

1. **Add a BRP verb that stamps a composition at a *cell*.** Drive 144 world-space cell coordinates.
   Camera-independent, re-runnable, and it makes the conversion reproducible rather than a one-off. This
   is the high-level action both papers advocate.
2. **Rewrite `site_67.map.ron` directly.** 144 stamps replacing 149 rows is a file transformation, and the
   plan has already *proved* the transformation is total and lossless for this map — no `lift`, `tip`,
   `paint`, every `patch: None`. That proof is exactly the condition under which a file rewrite is safer
   than a gesture replay. Load the result in the editor to verify, rather than using the editor to author.
3. **The pixel driver**, with the copy-and-diff safeguard already specified.

The plan takes (3) and attributes it to the author. That is a legitimate decision, but the plan argues
against nothing — (2) is strictly cheaper, has no mid-run-failure mode, and is directly enabled by
analysis the plan already did. It deserves an explicit argued rejection, not silence.

### 5.3 The item's own analysis falsifies the reason it was scheduled first

This is the finding worth the most.

`2026-08-09-composition-grammar-decisions.md` §1 justifies the whole ordering:

> "**#25 first because it is what produces more than four prototypes and real co-occurrence data.** A
> grammar learned from a single harvested room is degenerate in one of two directions (§3), and the fix
> is more examples, which is what #25 yields."

The plan's own analysis of `site_67` shows it does not yield that. After conversion the map is 139
`tile_floor` + 5 `tile_wall_n`. The observable adjacency relation is three pairs: floor–floor,
floor–wall_n, wall_n–wall_n. That is not "more examples" — it is the same single room in a new
representation, with a smaller vocabulary than the placements it replaced.

Karth & Smith, `10.1145/3337722.3341845`, on the end of the spectrum this lands on — the quote the
decisions doc §3 already carries:

> "A **LGG** learning strategy would say to only allow those adjacencies explicitly demonstrated in the
> source image. However, this highly-constrained alternative **might not allow any new output to be
> constructed that was not an exact copy of the source image.**"

And the PCGML survey, `10.1109/tg.2018.2846639`, names the structural condition in a section called
*Learning from Small Datasets*:

> "it is a commonly held tenet that more data is better; however, **games are likely to always be
> data-constrained.**"

So the accurate backlog entry is not *"the weights rest on co-occurrence data for two prototypes"* — it is
**the learned relation is degenerate in Karth & Smith's named direction: it can reproduce site_67 and
floor fields and nothing else.** And the consequence is not confined to FVS-R-7: **the decisions doc's
stated reason for putting #25 first has failed.** Either the rationale is restated — #25 is worth doing
for the representation change and the tile-gap census, and *not* for the training data — or a second map
enters scope.

The plan sees the edge of this (*"though see that item for how thin the output turns out to be"*) and does
not say the premise broke. It should, in the decisions doc, not only in the backlog.

### 5.4 The nine-missing-kinds census stands, and the corpus adds nothing

The enumeration — six wall-subsets of a cell up to rotation, three covered, three not, plus six shipped
descriptors with no tile — is arithmetic and I have no basis to dispute it. Nothing in the corpus speaks
to tile-set completeness for architectural kits; this is genuinely the project's own contribution.

---

## 6. Ordering — one dependency is inverted, and it isn't the one the plan argues

The plan's order is 1 → 2 → 3 → 4 → 5, with item 2 placed early because *"nothing depends on it yet."*

But the **achievable region** of the (enclosure, opening density) plane is a function of the tile alphabet,
and item 5's own conclusion is that the alphabet is missing nine kinds. A coverage floor of 20/400 bins,
committed against an alphabet that may not be able to reach 20 bins, fires on **alphabet poverty**, not on
generator bias — and nothing in the plan distinguishes those when it fires. §2 above is the same problem
seen from the metric-choice side.

**The amendment is not a reorder.** §4 exists so that a number cannot be chosen after seeing output; that
property must be preserved. Split the item:

- **2a, where the plan puts it.** Commit §4's method, both metric definitions, the fixed pre-declared
  domain grid, the three unambiguous rows (median enclosure `< 0.15`; median enclosure `> 0.95` with
  opening density `< 0.5`; `> 20%` non-convergence), the corrected fourth row as a *concentration*
  statistic per §1.2, **and the calibration rule** that will set its numeric floor — e.g. *"the floor is
  X% of the bins shown reachable by a constrained sweep."*
- **2b, immediately after item 5 and before the first grammar solve.** Execute the calibration, commit the
  number.

The number is then derived by a rule fixed in advance rather than chosen after looking at results, which
is the property §4 is actually protecting. The plan's current form protects the letter of that and loses
the substance, because an uncalibrated number is not falsifiable — it is only unchangeable.

---

## 7. Corpus hygiene — two items, both flagged on 2026-08-09, both still open

**(a) Smith & Whitehead is still a landing page, and item 2 is the item that needs it.**
`catalog_read("10.1145_1814256.1814260")` confirms the decisions doc §8 warning has not been actioned:
`pdf_path: papers/10/10.1145_1814256.1814260.html`, `total_pages: 1`, `chunks_indexed: 3` — for the
8-page *Analyzing the Expressive Range of a Level Generator*. Everything item 2 leans on is currently
being read second-hand through `pcgbook-ch12`. The catalog already carries a working URL:

```
http://www.soe.ucsc.edu/~ejw/papers/smith-pcg-2010.pdf
```

curl into `~/home-still/papers/10/`, `scribe_convert`, `distill_index`. This is the cheapest high-value
fix on the page, and it is upstream of item 2.

**(b) Lagae & Dutré is still not in the catalog.** `catalog_read("LD06AWTCECC")` → no entry. The
corpus-check doc §8.4 gave the exact ingest route on 2026-08-09 and it was never run. **Item 5 makes a
representation decision — the 50 mm east inset, moving a wall off the cell seam into the cell interior —
that is a member of exactly the family that paper treats.** I searched the corpus for edge-versus-interior
wall placement on a grid; the hits were fluid-simulation cut-cells and pathfinding discretisation, nothing
applicable.

---

## 8. Honest gaps

- **The 50 mm inset has no corpus support either way.** The one paper that would speak to it is not
  ingested (§7b). The plan's justification — preserves the 144-cell floor footprint, direction already
  recorded in the plan doc §2 — is internally coherent and I have no basis to confirm or challenge it.
- **Items 1, 4a, 4c–4g** are code-correctness work the corpus does not reach. Not vetted.
- **Mutation testing** is only weakly corpus-backed (§3).
- **`cargo` / BRP / Bevy specifics** — port collisions, `CARGO_TARGET_DIR` contention, the peer-session
  hazards, `git add` discipline — are operational and outside this sweep entirely. The `git add` explicit-paths
  rule and the never-drive-the-author's-instance rule both look right; that is judgement, not a citation.
- **I did not read the repository.** Everything above about `compose.rs`, `input.rs`, `keys.rs`,
  `site_67.map.ron` is the plan's own claim, taken at face value.

---

## Sources (all read from the local corpus this session)

- *Evaluating Content Generators.* PCG Book ch. 12 (authors not carried in the corpus metadata).
  `pcgbook-ch12-evaluating-content-generators` / `pcgbook_chapter12` — expressive range, hot-spot reading,
  sample-size-by-convergence, the metrics-far-from-inputs rule.
- Cooper, Seth (2022). *Sturgeon: Tile-Based Procedural Level Generation via Learned and Designed
  Constraints.* AIIDE. `10.1609/aiide.v18i1.21944` — **Expressive Range Coverage**: constrain-per-bin,
  19 of 36 found, 7 timed out.
- Horn et al. (2014). *A Comparative Evaluation of Procedural Level Generators in the Mario AI Framework.*
  FDG. `fdg2014_fdg2014_paper_14` — between-level compression distance as a diversity statistic distinct
  from per-level metrics; expressive-range heatmaps read for dense areas.
- Karth & Smith (2019). *Addressing the Fundamental Tension of PCGML with Discriminative Learning.* FDG.
  `10.1145/3337722.3341845` — LGG degeneracy; pattern learning from a single source.
- Togelius et al. (2018). *Procedural Content Generation via Machine Learning.* IEEE ToG.
  `10.1109/tg.2018.2846639` — *Learning from Small Datasets*; games as structurally data-constrained.
- Raistrick et al. (2024). *Infinigen Indoors.* CVPR. `10.48550/arxiv.2406.11824` — `SupportedBy`,
  `StableAgainst` as parent-child surface predicates.
- Tutenel, Smelik, Bidarra & de Kraker (2010). *A Semantic Scene Description Language for Procedural Layout
  Solving Problems.* AIIDE. `10.1609/aiide.v6i1.12398` — typed features.
- Hernández-Bécares, Costero & Gómez-Martín (2016). *An approach to automated videogame beta testing.*
  Entertainment Computing. `10.1016/j.entcom.2016.08.002` — raw input replay vs high-level actions;
  message-trace diffing.
- Aroudj & Ostrowski (2013). *Automated Regression Testing within Video Game Development.*
  `s40601-013-0010-4` — recorded mouse coordinates invalidated by UI movement.
- Mouret & Clune (2015). *Illuminating Search Spaces by Mapping Elites.* `10.48550/arXiv.1504.04909`;
  Fontaine et al. (2019), `10.1145/3377930.3390232` — archive/bin framing behind "coverage."
- Zhang, Harman, Ma & Liu (2019). *Machine Learning Testing: Survey, Landscapes and Horizons.*
  `10.48550/arXiv.1906.10742` — test adequacy, mutation testing.
- Sandhu, Chen & McCoy (2019). *Enhancing WFC with Design-level Constraints.* FDG.
  `10.1145/3337722.3337752` — carried forward from the decisions doc, unchanged by this sweep.

**Verified broken:** `10.1145_1814256.1814260` (Smith & Whitehead 2010) — HTML landing page, 1 page, 3
chunks. Do not quote.
**Verified absent:** Lagae & Dutré (2006), *An Alternative for Wang Tiles.* No catalog entry.
