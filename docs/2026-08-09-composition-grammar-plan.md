# Plan: a grammar over compositions

**Written to be executed from a cold start.** Every path, line number and claim below was checked
against the source on 2026-08-09.

**Lineage.** A four-move recommendation was written this day and then critiqued in
`2026-08-09-composition-grammar-decisions.md`. The critique's ordering, its validity-function
invariant and its falsification requirement are adopted here in full; §0 records the two places I
checked its open items and found them already closed, and the one place I think its correction
overshoots. This document is the executable form. Where the two disagree, this one has the line
numbers.

---

## 0. What I verified, including against the critique

| Claim | Verdict |
|---|---|
| §7 — `generate_from` already expands stamps so the solver sees them as unary constraints | **True.** `editor.rs:5236-5250`. `expand` propagates `Stamped::owned`; `grammar::solve` reads `owned` into `initial`. The solver half of fixed tiles exists. |
| §6 — the `wfc.rs` panic is in the substrate pass, not on move 1's path | **True.** `wfc::generate` panics on max attempts; `collapse_grid` returns `Option`; the editor's `G` goes `generate_from` → `grammar::solve` → `collapse_grid`. |
| §6 *open* — "does `generate_from` surface a non-converged solve usefully?" | **Closed, and the answer is yes.** `grammar.rs`: `collapse_grid(..).ok_or_else(\|\| "grammar: no arrangement satisfies what you have pinned… free some of them, or extend the example")?`. That reaches `status.problem`, which is sticky and red. There is no quiet sparse fill, so §4's heatmap has the precondition it needs. |
| §3 — "harvesting gives prototypes, not a grammar" | **True of the workflow, and it overshoots on move 1.** See below. |
| §8 — `10.1145_1814256.1814260` is landing-page boilerplate | Adopted without independent check; cite `pcgbook-ch12` instead. |

**Where §3 overshoots.** The correction is right that capture-once is not a workflow: `keep_as_group`
yields one `Composition` and no adjacency relation, and the iteration loop (capture → solve → reject →
capture) is not built. That withdrawal stands.

But move 1's adjacency does **not** come from examples. It comes from `composition::interface`, which
derives each tile's faces from its members' geometry. That is the critique's own Bad North precedent —
*"automatically detects alignment between the 3D geometry of neighboring tiles"* — and it means the
LGG over-constraining worry applies to **today's `Source::Learned`**, which reads adjacency off example
placements, and move 1 is the *fix* for it rather than a victim of it. Move 1 is therefore better
motivated than the four-move version argued, not worse.

**What is genuinely missing for fixed tiles**, located precisely: `Action::OwnToggle` (`O`) calls
`toggle_pin` on the placement under the cursor, and `redraw_stamps` gives stamped entities **no
`Placement` component** — deliberately, *"so the remove, move and clone tools cannot see them at
all."* A stamp therefore cannot be pinned by the one verb whose whole meaning is pinning. That is the
authoring half, and it is one component and one refusal, not a subsystem.

---

## 1. Order

```
#25  convert site_67's architecture to stamps (floors as well as walls)
 ->  §3  the falsification criterion, committed BEFORE any solve
 ->  §4  move 1: grammar from compositions, behind a validity-function seam,
         with fixed-tile authoring
 ->  §5  expressive range
 ->  #23 tags vs variants
```

#25 first, and the critique's reason is the right one: a grammar with four prototypes and no
co-occurrence data is degenerate whichever way it fails. Converting real architecture is what reveals
which tiles are *missing* (south walls, east walls, T-junctions, ends) and what supplies frequencies.

---

## 2. Step: #25 — convert `site_67`'s architecture to stamps

**Scope.** The five `site/wall` rows at `x = 0.0`, `z ∈ {7.5, 8.5, 9.5, 10.5, 11.5}`, **and the floor
rows they sit beside.**

**The floor half is not optional, and it was found by looking.** A tile carries its own floor, so
stamping onto already-floored ground leaves two coplanar floors — visible in a captured frame as a
changed surface, and it would z-fight in motion. The conversion replaces both rows with one stamp.

**The registration changes, deliberately.** The map centres walls *on* the tile seam (`x = 0.0` spans
`[-0.05, 0.05]`); tiles inset them (`−0.45` local, flush inside). A converted wall therefore moves by
50 mm. That is the correct direction — a straddling wall read as a composition sits on no face and
presents nothing — but it is a content change and should be looked at, not only diffed.

**Acceptance.** `cargo test --test site_tiles` green; the map loads; a captured frame shows the run
unbroken and no doubled floor. Record how many tile *kinds* the conversion wanted but did not have —
that number is the input to move 1 and the honest measure of whether four tiles was ever enough.

---

## 3. Step: the falsification criterion — committed before any solve

**This is the one thing that cannot be reconstructed later**, and the critique is right that a
threshold chosen after seeing output is not a threshold. From `pcgbook-ch12`: *"If you see five levels
that are impressive, among 50 that you choose to ignore or re-generate, what does that say about the
qualities of the content generator?"*

Two metrics, computed on the **solved cell grid** (not the placements), over ≥200 solves at varying
seeds on a fixed region:

**Enclosure** — the fraction of floor cells that lie inside a closed wall boundary. Computed by
flood-filling from the region's border across cells not separated by a wall face; cells the fill does
not reach are enclosed. Range 0–1.

**Opening density** — doorway tiles per enclosed region, as a mean over regions. Zero means sealed
boxes; unbounded means walls with no rooms.

**Committed thresholds, 2026-08-10. The approach fails if, over a run sized by the calibration rule
below:**

| Signature | Reading |
|---|---|
| median enclosure **< 0.15** | the solver makes wall confetti, not rooms |
| median enclosure **> 0.95** with opening density **< 0.5** | it makes sealed boxes nobody can enter |
| normalised entropy **`H / ln 36 < 0.25`** | one hot spot — uniform tiling or checkerboard, the two degenerate outcomes a small alphabet actually produces |
| **any single bin holds more than 50% of solves** | the same failure seen the other way; entropy alone passes a 70% hot spot with a broad tail |
| **> 20%** of solves return the `no arrangement satisfies what you have pinned` error | the alphabet is over-constrained; add tiles before judging the approach. **This row is a gate:** if it fires, the others are not interpretable |

The last row is the one the closed §6 buys us: a failed solve is *named and loud*, so it is countable
rather than being mistaken for a mediocre success.

**The third row was rewritten, and the original is worth keeping visible because the fault is easy to
repeat.** It read *"the 2-D histogram occupies < 5% of its populated bounding box"* — but a bounding
box computed from the populated cells is defined by them, so with every solve in one bin the box **is**
that bin and occupancy is 100%, the statistic's maximum. It is insensitive by construction to the case
it exists to catch. A second draft — *"fewer than 5% of bins occupied"* — fails differently and worse:
occupancy is blind to concentration, so 180 solves in one bin plus 20 scattered singly occupies 21 bins
and passes with 90% of the mass in the hot spot. **A hot spot is a concentration, so the statistic has
to be one.** Full argument, and why Cooper's constrained-coverage form is unavailable to this solver,
in `research/2026-08-09-composition-grammar-decisions.md` §4.3.

**The calibration rule, committed in place of X.** The achievable region of this plane is a function of
the tile alphabet, and #25's census finds nine kinds missing — so a floor picked today would fire on
alphabet poverty rather than generator bias, and nothing would distinguish them. From `pcgbook-ch12`:
*"generate **increasingly large amounts of content and visualize the expressive range, stopping when
the graphs begin to look the same**."* Fix the grid first (enclosure `[0,1]` × opening density `[0,4]`,
**6 ranges per dimension, 36 bins** — the granularity the corpus's own worked example used, and not
noise-dominated the way 400 bins would be at these sample sizes); run at doubling sizes until the
histogram stabilises; then set X from the max-bin share of a uniform distribution over the
*reachable* bins, times a stated multiple. Both halves get written down at calibration time.

**What is not negotiable is that the rule is written down before the first solve is looked at, and that
the number is derived from it rather than chosen after.** An uncalibrated number is not falsifiable; it
is only unchangeable.

**One metric is at risk on ch12's own test.** *"Strive to choose metrics that are as far as possible
from the input parameters… a metric highly correlated to an input parameter can only ever provide
confirmatory results."* Under a four-tile alphabet, **enclosure** is very nearly a restatement of how
many wall and corner tiles the solver was permitted to place; **opening density** is the safer of the
two. Not settleable before the alphabet is fixed — flagged, and revisited when it is.

---

## 4. Step: move 1 — a grammar over compositions

**Goal.** `grammar::learn` gains a sibling that builds prototypes from the composition set instead of
from cell-sized placements, so a kit whose meshes are never cell-sized becomes solvable.

### 4.1 The invariant: adjacency goes through a substitutable validity function

Adopted from the critique, and it is a constraint on how the commit is written rather than a task:

> Karth & Smith (`10.1145/3337722.3341845`): *"any arbitrary adjacency validity function can be
> substituted here… it can act as the whitelist for the constraint domains **without changing the WFC
> solver itself**."*

So: the new builder must compute `support` through a named function of the shape

```rust
fn agrees(a: &Interface, b: &Interface, dir: Dir) -> bool
```

and **must not** compare `Interface::faces` inline. Edge-matching is one implementation; corner
matching is another; the corner question (still blocked on Lagae & Dutré) stops gating anything.

### 4.2 What the builder does

- One prototype per `Bounded` composition per quarter turn, deduplicated on identical face signatures
  — the rule `learn` already applies, which is what keeps a symmetric tile from spending four slots.
- Faces come from `composition::interface`, already division-independent since step 2's band form.
- **Weights come from #25's converted map** — count each composition's stamps. This is why #25 is
  first: without it every prototype has weight 1 and the solve is uniform by construction, which
  would trip the histogram threshold above for a reason that has nothing to do with the design.
- An `Anchored` composition is skipped, not refused: it claims no tile and is furniture.

### 4.3 Fixed-tile authoring, folded in

The solver half exists (§0). The missing half is that a stamp cannot be pinned. Give stamped entities
what the pin verb needs to see them — and *only* that, since `redraw_stamps`'s note is right that the
move/remove/clone tools must not reach them.

Sandhu, Chen & McCoy (`10.1145/3337722.3337752`) is why this is inside move 1 rather than after it:
they measure *"a conflict rate around 60% for a map size of 100 tiles"* for unconstrained area
propagation, concluding it *"is best used as a constraint for design time rather than runtime."*
Without pinned boundaries the first real solve is the case they measured as failing, and the
falsification criterion would record that as the approach failing rather than the setup being wrong.

### 4.4 Acceptance

- `cargo test --workspace` green; **no golden may move** — this is editor-side.
- A unit test that `agrees` is the only place faces are compared, so the seam cannot rot.
- A test that a pinned stamp survives a solve untouched, and that an unpinned one does not.
- The editor's `G` (Learned) is unchanged; the new source is a third `Source` variant, so the existing
  behaviour is still reachable and comparable.

---

## 5. Step: expressive range

Generate 200+ solves, compute §3's two metrics, plot the 2-D histogram, and read it against the
thresholds **as written**. Then, and only then, look at the maps.

---

## 6. Then #23 — tags versus variants

The better argument, kept from the critique because it is stronger than the cross-product one I gave:
**under tags an absent mesh is a missing renderer and the functional layer still solves; under
variants the axis cannot be expressed at all.** The Site kit having no wall-mounted piece is the
evidence, and it says do not wait on the kit.

---

## Non-goals while executing §2–§5

- **Do not retire the positional `Mount` variants.** Furniture uses them and furniture is deliberately
  not gridded. This was in the original step 5's tail and should move further out, not nearer.
- **Do not grid `Placed::at`.** Structure is solved; dressing is continuous. The corpus is unanimous.
- **Do not add the occupancy test inside a composition** without `noparent` scoping in the same
  commit.
- **Do not touch `wfc::generate`.** It is the dungeon substrate and its panic is correct there.

## Housekeeping

`scripts/mirror_crates.sh` refuses while `crates/bevy_autogib/` is untracked, so recent commits are
unmirrored. That is the script working — refusing beats forcing past a dirty tree — but it clears only
when the other agent commits, so check rather than assume.
