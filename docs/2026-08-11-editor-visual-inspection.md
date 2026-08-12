# Visual inspection of emerge-mapper over BRP — 2026-08-11

Driven entirely through `bevy_debugger/input` and `bevy_debugger/screenshot` on `BEVY_BRP_PORT=15799`.
No OS keyboard, no screen capture, window never raised.

Project: `assets/emerge/site`, map `site_67`, kit `site`. Map bounds 32 x 32 m = **1024 cells**.

---

## D1 — BLOCKER. The enclosure constraint optimises a metric that counts walls as floor, and the
## generator has found the degenerate answer: a wall thicket.

**Evidence:** `shots/01_cmdG_rooms.png`, `shots/02_zoomed_out.png`. One `Cmd+G` turns the 12x12 slab
into a uniform lattice of wall fragments filling all 1024 cells. There are no legible rooms anywhere
in the region.

**Mechanism.** `range::Faces::floor(p)` is `p != 0` — *every* prototype but `Empty` is floor,
**a solid wall tile included** (`range.rs:78`). Enclosure is
`|floor and not outside| / |floor|`. A wall tile blocks all four of its own seams, so a wall cell
whose neighbours are also walls is unreachable from the border and therefore counts as *enclosed
floor*. The cheapest way to satisfy "at least 256 of 1024 cells enclosed" is therefore **to fill the
region with walls**, not to build rooms.

**This is Goodhart, and the criterion predicted it.** §4.1 of the decisions doc flagged enclosure as
*"highly correlated to an input parameter"* and therefore able to *"only ever provide confirmatory
results"*, and named opening density as the safer metric. Turning that metric into a *target* converted
a measurement weakness into a generation defect.

**What the test suite says, and why it did not catch this.** `range::measure` agrees the output is
enclosed — 0.462 median over 2,048 solves — because the constraint and the metric share the same wrong
predicate. Every test I wrote checks the constraint against `range::measure`, so they agree with each
other and both disagree with the picture. The diagnostic I added (`expressive_range -- rooms <seed>`,
counting only the real floor tile as floor) reported 0.31-0.75 at 12x12 and was the only signal — I
read it as reassuring when it was the warning.

**Not a regression in the metric.** The predicate is pre-existing and the criterion is pre-registered;
what is new is a constraint that optimises against it.

**Candidate fix, NOT applied — it changes generated output and a pre-registered measurement, so it is
the author's call.** Have `enclosure_rules` count only cells holding a real deck tile as enclosable
(`enclosed[c] -> place[c][deck]`), leaving `range::measure` untouched. That constrains the thing the
word "room" means without amending a criterion after seeing its output.

---
## Scope limit — UI panels could not be inspected, and that is by design

`bevy_devshot` (the whole-frame path, the only one that sees the UI tree) returns a **fully black
frame** while the window is unfocused — the documented behaviour, measured previously at 7,188
distinct colours focused against 1 unfocused. Raising the window would steal focus on a machine
somebody is using, which the project forbids and which is the whole reason the BRP path exists.

So everything below is what the **mirror camera** sees: the world. Status lines, the problem banner,
the palette, the key census and every panel are **not covered by this pass** and would need a human at
the keyboard, or a session where raising the window is acceptable.

---
## D2 — An agent cannot tell "refused" from "did nothing". The generate path never logs.

**Evidence:** `shots/04_G_learned.png`, `shots/05_shiftG_declared.png` — both identical to the
untouched map. Bare `G` and `Shift+G` produced no visible change, no log line, and no BRP-observable
difference. Both are almost certainly *correct refusals*: `learn` reads `map.placements` and site_67's
content is entirely stamps, and `declared` refuses tiles that are not the grid's size, which the site
kit's 0.1 x 1.0 m walls are not.

But nothing anywhere says so. All three refusal exits in `generate_from` are bare
`state.status.problem(e); return;` with **no `error!`** — while `redraw_stamps` two thousand lines
earlier pairs exactly the same `status.problem` with an `error!` (`editor.rs:3438-3439`). So the
message exists only in a UI panel, and the UI panel is the one thing neither BRP capture nor an
unfocused devshot can see.

**Why this matters more than it looks.** The project's own rule is that a failure should be *"named and
loud"*. Here it is named and silent. An agent driving this editor — the documented workflow — cannot
distinguish "the grammar refused, and here is why" from "the keystroke never arrived", which are
opposite diagnoses.

**Confirmed later, by accident.** The `drive.request: stamp` verb logs `editor.status.line()`, and
because a problem outlives every subsequent note, that call surfaced the message `Shift+G` had left
sitting there twenty minutes earlier:

> declared grammar: every tokened piece is the wrong size for a 1 m cell, so a solve would place them
> at a spacing unrelated to their extents — `site/column` is 0.5 x 0.8 m, `site/wall_corner` is
> 0.22 x 0.22 m, `site/wall_doorway_wide` is 0.4 x 2 m, `site/wall_doorway` is 0.46 x 2.06 m,
> `site/wall_header` is 0.1 x 1 m, `site/wall_low` is 0.2 x 1 m, `site/wall_window` is 0.2 x 2 m,
> `site/wall` is 0.1 x 1 m. A tile grammar needs tiles of the grid's size; generate from the map
> instead, or author a kit on the cell.

So `Shift+G` refused **correctly and with an excellent message** — it names every offending piece, its
measured size, and two ways out. None of it was reachable. The information exists; only the channel is
missing.

**Fix:** add `error!` beside the three `status.problem` calls in `generate_from`, matching
`redraw_stamps`. One line each, no behaviour change for a human.

---
## Verified working

- **All four tabs render.** Map (slab + wall), Tiles (descriptor with its edge-token lattice overlaid
  in cyan/magenta), Compose (carousel), Anim (rigged character). `shots/06`, `07`, `08`, `09`.
- **The Anim preview is actually playing**, not a frozen pose — three captures 1.5 s apart are three
  distinct frames. `shots/08a`, `08b`, `08c`.
- **The composition carousel cycles and `O` is an exact inverse of `P`** — next, next, prev returns a
  byte-identical frame. At index 0 the miniatures sit only to the right; past it they appear on both
  sides, which is the documented behaviour rather than the layout bug it first looked like.
  `shots/07`, `10`, `11`, `12`.
- **`J` cycles the grid rung** and all four rungs redraw distinctly. `shots/13_grid_rung_1..4`.
- **The armed-composition preview works** (commit 737ec87), and this took three attempts to see. At
  the default zoom a 1 m tile is ~12 px and the ghost is invisible; armed and disarmed frames differ
  by hash but not to the eye. Captured with `region` + `zoom: 4.0` it resolves into a thin floating
  header bar — which is exactly right, because `site/tile_doorway_n` *is* a lifted `wall_header` over
  open air. `shots/22_armed_zoom.png`. **Nearly filed as a defect; it is not one.**
- **`drive.request` verbs all work**: `map`/`tiles`/`compose`/`anim`, `arm` (toggles, and logs the
  status line), `stamp` (144 -> 145 stamps at world origin), and undo returns it.
- **Undo/redo is sound across the generate.** `Cmd+G` 144 -> 749 stamps, `Cmd+Z` back to 144, several
  times, with the row counts matching exactly each way.

## Not reproduced / no evidence either way

- `dump` applies but printed no hierarchy on either attempt. Either the staged-preview tree is empty
  in the states I drove it from, or the output goes somewhere the log does not. Worth one look.

---
## D3 — the whole stamped picture is rebuilt on keystrokes that change nothing

**Evidence:** 23 `redrew` lines in one short session, **18 of them rebuilding the identical
`152 rows from 145 stamps`**. They fire on `R`/`T`/`Y`/`U`/`[`/`]` with no piece under the cursor —
verbs which `turn_under_cursor` provably abandons *before* `project.dirty = true` (`editor.rs:4651-4662`).

**Mechanism.** `redraw_stamps` is gated on `project.is_changed()`, and Bevy's change detection flags a
resource when a system takes `ResMut<T>` and dereferences it — not when it actually mutates. The key
dispatcher takes `ResMut<Project>` for every one of these verbs, so a keypress that correctly decides
to do nothing still reads as a change.

**What it costs.** `redraw_stamps` despawns every `StampInstance` and re-derives the whole picture
through `composition::expand` + `stack::resolve_y`. At rest that is 152 rows. **After a `Cmd+G` it is
1,616** — so, post-generate, a keystroke that does nothing tears down and rebuilds sixteen hundred
entities. Not a correctness bug; a real interactivity one, and it grows with the map.

**Fix:** gate on `project.dirty` (which the code already maintains honestly) rather than on
`is_changed()`, or take `Res<Project>` where the verb has not yet decided to write.

---

## D1's fix, tested rather than proposed

The candidate fix was run as an experiment and **fully reverted** (`git status` clean, 54 tests green).
It changes one clause family in `enclosure_rules`: a cell is enclosable only if its prototype walls
**nothing** — real open deck — instead of "anything but `Empty`". `range::measure` is untouched, so no
pre-registered criterion is amended.

**Per-seed, counting only real floor as floor** (the diagnostic `expressive_range -- rooms <seed>`
prints):

| seed | shipped | with the fix |
|---|---|---|
| 7 | 0.500 | **1.000** |
| 3 | 0.750 | **0.974** |
| 11 | 0.727 | **0.952** |
| 42 | 0.308 | **0.947** |

And the drawn grid changes character completely — a contiguous block of open floor with walls around
it, where before there were scattered wall glyphs.

**Full sweep, 2,048 solves, against the same committed rows:**

| | shipped | with the fix |
|---|---|---|
| median enclosure | 0.462 | 0.630 |
| H / ln 36 | 0.536 | 0.440 |
| max bin share | 39.1% | **50.0%** |
| clamped above opening 4 | 966 (47%) | **1,244 (61%)** |
| verdict | no row fires | no row fires |

**So it fixes the thing it was aimed at and sharpens a different problem.** Real rooms appear, and
max-bin share lands on *exactly* the 50% ceiling — because more than half the mass is now in the
clamped opening-density column. Row 4b would be one solve from firing, on an artifact rather than on
concentration. That is caveat 2 from §9 of the expressive-range doc, now load-bearing: **the `[0, 4]`
opening-density domain is too low for this generator**, and widening it is a §4 amendment that must be
pre-registered before the run it governs, not after.

**Recommendation:** take the fix, and pre-register a wider opening-density domain in the same change,
before re-measuring.
