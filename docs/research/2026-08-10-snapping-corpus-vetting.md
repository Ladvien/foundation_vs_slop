# The snap ladder, vetted against the snapping literature

**Date:** 2026-08-10
**Question:** does `grid::SnapLevel` (shipped in `a10cadf`) survive the two papers that actually cover
snapping in design tools?
**Sources:** Bier (1990), *Snap-dragging in three dimensions*, I3D — `10.1145/91385.91446`; Ciolfi
Felice, Maudet, Mackay & Beaudouin-Lafon (2016), *Beyond Snapping: Persistent, Tweakable Alignment and
Distribution with StickyLines*, UIST — `10.1145/2984511.2984577`. Bier & Stone (1986) not read; the
1990 paper supersedes it for everything below.

**Why this was needed:** four corpus sweeps for snapping returned neural shape editing, multigrid
Poisson solvers and cascaded light propagation volumes. It is a graphics/PCG corpus with no HCI
direct-manipulation holdings, so the ladder shipped on internal coherence alone.

---

## 0. Verdict

| Decision in `a10cadf` | Verdict |
|---|---|
| Corner rather than centre | **Supported, and for a reason I had not got.** |
| Drawn grid follows the active rung | **Directly supported** — it is the papers' *predictability* finding. |
| Solver-visible pieces locked to the tile | Untouched; that rests on ch11, and still does. |
| **Rungs on held modifiers** | **Challenged.** Both papers cycle modes rather than hold them. |
| Geometric footprint as the snap box | **The one real defect.** Named explicitly by StickyLines. |

Two capabilities the ladder does not have and the literature says matter: **snapping to what is
already placed**, and **tweaks that persist**.

---

## 1. What the corner rule gets right, and the better argument for it

I argued the corner from arithmetic: `cell_centre` is `min + (c + 0.5) * TILE`, so the phase differs
per footprint and only a corner rule is correct at every rung. That holds. But StickyLines supplies the
*user-facing* reason, from twelve interviews:

> "I am aligning with respect to what? Does the selection order matter?" (P9)
> "What is the reference? Is it the width of the page?" (P10)

Their **lack of control** finding is precisely this: tools do not reveal the reference point, so results
cannot be predicted. A centre-snap on a grid drawn at cell boundaries is that failure exactly — the
reference (the centre) is invisible and sits between the lines you can see. The corner rule is
defensible not only because the arithmetic works but because **the reference is a thing an author can
point at.**

The same finding endorses the grid change:

> "a human designer may become frustrated or confused if the computer consistently acts as though it
> is not following the model that the human designer has in her head." — `pcgbook-ch11`, and
> StickyLines' *control* problem is the same claim with an experiment behind it.

---

## 2. The challenge: held modifiers are the wrong shape for a mode

**Bier cycles, and does not hold.** Snap-dragging's gravity modes — points-preferred, lines-preferred,
faces-preferred — are changed by *keyboard commands*: "Cycle Forward (Backward) through the Three
Gravity Functions, Toggle Gravity On and Off". Nothing is held down. Of 44 commands, the modal ones are
all latched.

StickyLines' designers say why holding is costly:

> All the designers and one developer make extensive use of the keyboard to align and distribute
> objects, not only because it is faster, but also because "there are too many options and menus" that
> clutter their screens and make them 'lose focus'.

A rung held with Shift is fine for a single nudge and bad for a dressing session, which is the case the
lower rungs exist for. **This does not invalidate the modifiers** — a transient override is genuinely
useful, and Bier's *gravity toggle* is exactly that shape. It says the ladder wants a **latched rung as
well**, and the editor already has the verb for it: `CycleGrid` cycles the drawn grid and is the
natural place to cycle the rung, now that the two are the same thing.

Recorded as **FVS-R-19**.

---

## 3. The defect: the geometric box is not the visual one

This is the finding that costs something. StickyLines, verbatim:

> Alignment and distribution commands use the **geometric center** of objects, but sometimes this does
> not match the object's **visual center**. Seven participants had recently used commands to align what
> they referred to as 'irregular' or 'weird' shapes... **All were forced to fine-tune the result** to
> make it aesthetically pleasing. We call such edits **tweaks**. To our knowledge, current tools
> completely ignore such tasks.

`brush_span` reads `extent.footprint` — the measured geometric box. For any piece whose art does not
fill its box, the corner snap is *arithmetically* right and *visually* wrong, and the author has no
recourse but to hold Alt and place free, which throws away the lattice entirely.

**This project has already met the phenomenon and written it down without naming it.** `policy.rs`, on
seating:

> `site/wall` is 0.1 m thick and sits flush at −0.45, which is not a multiple of 0.125 either, **because
> art is authored to look right rather than to tile.**

That is StickyLines' visual-vs-geometric extent, in this repo's own words, about this repo's own kit.

Their fix is the one worth copying, and it is not "snap to the visual centre" — it is to make the
adjustment **a first-class, persistent, reusable thing**:

> Tweaks reify the action of adjusting an object's position... They are first-class objects that can be
> edited, **copied onto other objects**, and deleted.
> Users can also tweak the **bounding box** of an object... **without affecting the object itself.**

In this schema that is a per-descriptor snap box — an authored offset/extent used *for snapping only*,
living beside `align.pivot` and `align.y_offset`, which are already "adjust how this art sits" fields.
Authored once per piece rather than tweaked per instance, which is the cheaper half of their idea and
fits a kit-based editor better than a poster editor.

Recorded as **FVS-R-20**.

---

## 4. The capability the ladder does not have: snapping to what is already there

Bier's thesis, and the sentence the whole technique rests on:

> Snap-dragging is a synthesis of the best properties of **grid-based systems, constraint networks, and
> drafting**.

`SnapLevel` is the grid-based corner and nothing else. Snap-dragging's gravity snaps the cursor to
**vertices, edges, faces and their intersections** of objects already in the scene, preferring points,
then lines, then faces, then a default plane. StickyLines reaches the same place by a different route —
persistent guidelines objects attach to — and both report the same user need: *align this to that*,
where *that* is a thing already placed, not a lattice position.

For a tile editor this matters less than it looks: on a fixed lattice, "aligned with the tile next to
it" and "on the lattice" are the same statement, which is why the ladder is enough for tiles. It
matters for **dressing**, which is exactly what the lower rungs are for — a lamp against a table edge is
not at a third of a cell, it is at *the table's edge*.

Two details worth stealing if this is ever built, both cheap and both about ambiguity:

- Gravity returns **an ordered list, not a best answer**, and the user cycles it: "the user can cycle
  through the objects near the cursor line. Thus snap-dragging gravity computes not only a best point
  but also an ordered list of close points." That is the answer to "it snapped to the wrong thing".
- The **default plane** — when nothing is near, motion falls back to a plane parallel to the screen,
  and the small jumps between plane and object are deliberate feedback: they "help the user tell when
  gravity has drawn the skitter to a new object."

Not scheduled. The tile case is served, and this is a large feature whose value is in the dressing
pass. Noted here so the next person does not re-derive it.

---

## 5. What I am not changing on this evidence

**The thirds default.** Neither paper speaks to divisor choice; both are about *what* you snap to, not
*how finely*. That remains the author's call, settable per project.

**The tile lock on solver-visible pieces.** ch11's expressiveness constraint is a statement about
mixed-initiative generation, which neither of these papers is about. Unchanged.

**The corner rule.** Strengthened, not weakened — §1.

---

## 6. Honest gaps

- **Bier & Stone (1986) not read.** The 1990 paper covers gravity, alignment objects and
  transformations in more detail; the 1986 one is the two-dimensional original.
- **Neither paper is about tile grids.** Both are about free-form graphical editing — posters, 3-D
  scene composition — where there is no lattice at all. Their *predictability* and *tweak* findings
  transfer cleanly because they are about the reference point, which exists either way. Their
  *persistence* finding transfers weakly: on a fixed lattice the lattice is the persistent relationship.
- **The experimental result does not transfer.** StickyLines' 40% faster / 49% fewer actions is
  guidelines-vs-commands in a poster editor; nothing here measures a tile editor's snap.
- **Corpus status:** both read from text supplied directly, not from an ingested `distill` stem, so
  quotes here are verifiable against the papers but not yet against a corpus chunk.
