# The kitbash editor — owned tiles, and WFC around them

**Status:** design. Nothing below is built. The Site editor it extends is on `main`
(`src/site_editor/`, `FVS_SITE_EDITOR=1` + F7).

## The idea, in one line

The author places the tiles that **must** be a certain way and marks them **owned**; WFC and the
placement rules fill in everything else around them, and re-fill it on demand without ever disturbing
what was owned.

## Why this is the right shape

Site-67 currently has a wall down the middle of it. `scripts/gen_site67.py` owns `areas`, `floor`,
`walls` and `doorways`; the editor owns `props`, `cells` and `spawns`; the two never meet, because an
in-game tool writing the generated half would recreate exactly the drift that script was written to
eliminate.

That is the central problem of this field **dodged, not solved**. Smelik, Tutenel, de Kraker & Bidarra
(*Integrating procedural generation and manual editing of virtual worlds*, 2010) name it first of
three open issues:

> 1. how to preserve manual edit actions on a terrain feature throughout procedural re-generation?
> 2. how to balance user control versus automatic model consistency maintenance?
> 3. how to integrate both procedural and manual operations in the same iterative workflow?

and **sketch** three facilities borrowed from image editors: **Locking** (this is immune to
operations), **Scoping** (bound an operation's blast radius), **Grouping** (move the road and its
buildings come along).

**Read that as aspiration, not prior art.** §5.3 introduces them as *"possible facilities, which are
inspired from image processing software, but have more advanced and complex semantics"*, in a paper
whose own assessment is that integrating procedural generation with manual editing is *"so far as good
as unaddressed"* — with preserving manual operations through regeneration called out as particularly
difficult and the proposed fix a sketch. Nobody shipped this triad; it is a useful naming of the
problem, and the design below is ours to prove. (Correction recorded in
`2026-08-03-forge-plan-review.md` §4.)

Alvarez et al. (FDG 2018) do implement the first of the three as a **lock brush**: the designer locks tiles, the
room subdivides into mutable and immutable zones, and every generated suggestion preserves the locked
ones. Their genotype stops being one-gene-per-tile and becomes a tree over zones — *"instead of
manually editing a room first to later generate appealing solutions based on it, the user can now start
from a suggestion, selecting parts of it that look promising that are kept through subsequent
generations."*

"Owned" is that lock. This document uses **owned** rather than *locked* because it says the useful half
out loud: the author has taken responsibility for this tile, and the generator must route around it.

## Ownership is already a WFC primitive

This is the part that makes the feature cheap rather than speculative.

Karth & Smith (*WaveFunctionCollapse is Constraint Solving in the Wild*, FDG 2017) — cited throughout
`src/wfc.rs` — establish WFC as finite-domain constraint solving. An owned tile is therefore not a
special case bolted onto the solver: it is a **unary constraint**, a cell whose domain is narrowed to
one prototype before propagation begins.

`src/wfc.rs` already takes exactly that:

```rust
pub fn collapse_grid(w, h, &weights, &support, &initial, seed) -> Option<Vec<usize>>
```

`initial` is the per-cell domain bitmask, and the module already uses it for the boundary rule (*"a CSP
unary constraint; Karth & Smith 2017"*). There is already a test:

```
initial_domains_restrict_output()
    // A narrowed initial domain is honored: pinning cell 0 to prototype 2 must yield it there.
```

So the generator half of this feature is: build `initial` from the owned set, call the existing
collapser. No new solver, no fork of the existing one.

## What gets built

### 1. Ownership as authored data

A new list in `site67.ron`, written by the surgical writer that already exists
(`src/site_editor/source_map.rs` — one `key: <value>` span rewritten per edit, comments preserved,
byte-identical no-op save):

```ron
owned: [
    ( cell: ( 30, 12), piece: WallDoorway, yaw: 90.0, why: "the cell block's only entrance" ),
]
```

`why` is a **reason string, never a bool** — the same call `PropPlacement::waive` already makes, and for
the same argument: *"a `bool` would let 'I could not be bothered' and 'this deliberately overhangs the
counter' look identical in the diff."* An owned tile constrains a generator; six months later, the only
thing that can say whether it still should is the sentence the author wrote.

### 2. The generator moves into Rust

`gen_site67.py`'s derivation is ~120 lines of set logic — boundary detection, convex corners, wall yaw,
connectivity flood-fill — and it must move into the game so the editor can re-run it live. **The Python
is deleted, not left beside it.** Two sources of truth is the failure that script's own docstring exists
to describe.

Ported, `regenerate(rooms, corridors, doorways, owned) -> (areas, floor, walls)` becomes callable after
every structural edit, which is what makes dragging a room wall a live operation instead of a build step.

### 3. The three facilities

* **Own** — the brush. Paint cells owned; they pin their prototype and survive every regeneration.
* **Scope** — regenerate *this room*, not the Site. Bounds the blast radius so a bad roll costs one room.
* **Group** — a room's dressing is tied to its walls, so moving a room moves its furniture. This is the
  feature that would have made `scripts/migrate_site67_props.py` unnecessary; its docstring names the
  failure it was written to avoid — *"which is how a chair ends up facing a wall it used to face away
  from."*

### 4. Snapping: sockets, with a modifier for free placement

`kit_ozea.ron` gains named attachment points per piece — wall ends, table edges, floor seams. A dragged
piece snaps to a compatible socket in range. This reuses the vocabulary the manifest already has:
`surfaces` (what a piece *offers*) versus `affordances` (what it is *for*), the split Tutenel et al.
(2010) draw and `placement::manifest` already encodes, with the warning that folding them together is
"exactly the 'prop rests on a bed' bug."

Holding **Ctrl/Cmd** suspends snapping and places on free XYZ.

> One path, stated rather than discovered: sockets and free placement are two ways of *naming a
> position*, both feeding the single `move_prop` / `place` operation that is the only thing able to
> change a record. The modifier is an input mode, not a second write path. Free placement is what a
> socket set can never anticipate, and refusing it would mean the tool can only build what the kit
> author already imagined.

## Stages

| Stage | Work |
|---|---|
| A | `owned:` schema + brush + overlay; ownership honoured by nothing yet (data first, so it can be authored and reviewed before it constrains anything) |
| B | Port `gen_site67.py` to Rust, delete the Python, pin the port against the current `site67.ron` byte-for-byte |
| C | Wire owned cells into `collapse_grid`'s `initial`; **Scope** = regenerate one room |
| D | Sockets in the kit + Ctrl/Cmd free placement |
| E | **Group**: room ↔ dressing, so structure edits carry furniture |

Stage B's pin is the interesting test: the Rust port must reproduce the shipped 890 wall rows and 58
floor runs exactly, which turns "did I port it right" into a diff rather than a judgement.

## Verification

* **Byte-pin the port** (Stage B) — Rust `regenerate()` against the shipped `site67.ron`, exact.
* **Ownership is honoured** — regenerate a room 100 times under different seeds; every owned cell holds
  its prototype in all 100. This is the property the whole feature rests on and it is cheap to assert.
* **Regeneration preserves dressing** — props inside a regenerated room keep their positions, and
  `check_prop_placements` reports no *new* faults it did not report before.
* **The writer still cannot damage the file** — the existing 15 tests in `tests/site_editor.rs` extend
  to the `owned:` list.
* **Visually, every time.** Three of this editor's bugs were invisible to 31 green suites and only a
  rendered frame found them — a blanked framebuffer, a leaked full-screen title node, a duplicate-`Node`
  panic. Render a frame and look.

## Open questions

1. **What is the unit of ownership** — a cell, or a placed piece? A cell is what WFC constrains; a piece
   is what an author points at. Probably cells, with the brush painting the cells a piece covers.
2. **Does ownership survive a kit swap?** `kit_greybox.ron` exists to prove the kit is replaceable. An
   owned tile names a `SitePiece`, which is kit-independent, so it should — worth asserting.
3. **Should the RL/QD search ever see this?** No, on the current argument: `site::layout`'s header keeps
   `site67.ron` out of `config.ron` precisely so the search cannot evolve the hub. Ownership is level
   data and inherits that exemption. Worth restating here because "WFC generates it" is the exact
   phrasing that would make someone wire it into the genome.

## Bibliography

* Smelik, Tutenel, de Kraker & Bidarra, *Integrating procedural generation and manual editing of virtual
  worlds* (2010) — `10.1145/1814256.1814258`. The three open issues; Lock / Scope / Group as *proposed*
  facilities, never implemented there. See `2026-08-03-forge-plan-review.md` §4.
* Tutenel, Bidarra, Smelik & de Kraker, *A declarative approach to procedural modeling of virtual
  worlds* (2010) — `10.1016/j.cag.2010.11.011`. Fine-grained control; surfaces vs affordances.
* Alvarez et al., *Empowering quality diversity in dungeon design with interactive constrained
  MAP-Elites* / lock-brush work (FDG 2018) — `10.1145/3235765.3235810`.
* Karth & Smith, *WaveFunctionCollapse is Constraint Solving in the Wild* (FDG 2017) — the reason an
  owned tile is a unary constraint rather than a special case.
* Liapis, Yannakakis & Togelius, *Sentient Sketchbook* (FDG 2013) — real-time evaluation beside the
  canvas, already the basis of the editor's live rules panel.
