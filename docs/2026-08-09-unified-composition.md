# One way to compose pieces in a grid unit

A design for replacing the two-and-a-half mechanisms that currently decide what rests on what. Written
after measuring the current behaviour rather than from memory; every number below was read out of the
code or the shipped kits on 2026-08-09.

**Status: design, not built.** The blast radius is the schema, so this wants agreement before code.

---

## 1. What is there now, and why it is two mechanisms

Ask "what holds this piece up" and the answer arrives by one of two completely different routes.

**Relational — the good one.** `Mount::OnSurface { class }` plus `Placed::on` plus `Offers::surfaces`.
A lamp declares it needs a `"worktop"`, a table declares it offers one, and the placement records
`on: "table@4"`. `stack::resolve_y` resolves the host first and sets the lamp's Y to the host's *drawn*
top — `placed_height` = extent × `scale` × `stretch_y`, so a table measured 0.796 m and scaled 1.2
presents at 0.955 m. Move the table and the lamp follows, because the map stores a **reference, never a
height**. Loops are detected and named. This is Tutenel et al.'s *semantic class* relation, and it
works.

**Positional — the other one.** `OnFloor`, `OnWall { height }`, `OnCeiling`, `InOpening`, `Tiled`,
`Overlay { on }`. `stack::datum` resolves these against the **map datum**: a literal in the descriptor
plus `map.origin.1`. No host is recorded. Nothing moves together.

So a sconce is not attached to its wall. `wall_light` is `OnWall { height: 1.8 }`, which means 1.8 m
above the *map's floor* — not 1.8 m up whatever it is fixed to. Delete the wall and the sconce hangs
there, and nothing reports it.

**And a third thing, tangled into both.** `stack::blocking` decides whether two pieces contest space.
It is a purely 2-D plan test — `plan_box` discards height entirely — gated by `same_layer`, a
hand-maintained match over pairs of mount variants:

```rust
(None | Some(OnFloor), None | Some(OnFloor)) => true,
(Some(Tiled), Some(Tiled))                   => true,
(Some(OnWall { h1 }), Some(OnWall { h2 }))   => (h1 - h2).abs() < 1e-3,
(Some(OnSurface { .. }), Some(OnSurface { .. })) => a_on == b_on,
_ => false,
```

It is consulted by exactly two callers — `drive_place` and `stamp_set`. Flood fill and `G` never ask.

### Three defects that follow from this, all measured

1. **`site/wall` and `site/floor` are both `OnFloor`**, so they are the same layer, and a 0.1 × 1.0 m
   wall placed anywhere inside a 1.0 × 1.0 m floor tile overlaps it by far more than the 1 cm
   `OVERLAP_EPS`. Hand-placing a wall onto a floor tile is refused. It only works today because the
   two callers above are the only ones that ask. The site kit has **zero** `Tiled` entries — the
   stratum that was meant to keep floor out of the way.
2. **`Tiled` is a dumping ground.** Correct on `floor_grate` and `floor_light`; also on `bin`,
   `pouffe` and four `barrel_*`. Those pass through every `OnFloor` piece, so a chair can stand inside
   a barrel and nothing refuses.
3. **The pair you would call "attached" is the pair nothing links.** No wall-mounted piece has a host,
   so no orphan can be reported.

---

## 2. What the literature already says

`10.1609/aiide.v6i1.12398` — Tutenel, Smelik, Bidarra & de Kraker, *A Semantic Scene Description
Language for Procedural Layout Solving Problems*, AIIDE 2010. Already cited by this codebase for the
class relation; the part that matters here is **features**:

> *"classes also contain **features**, defined as generic shapes associating semantics to an object's
> model. For example, most physical object classes have a **front, back or a top feature** defined, and
> a bookcase has **storage features defined on every shelf**."*

and the rule that falls out of them:

> *"Feature types can have **embedded layout semantics**, e.g. **off limits** features cannot overlap
> any other features and **clearance** features can only overlap other clearance features."*

That is the whole of `same_layer`, derived instead of tabled. And the same paper notes that one
mechanism serves both halves of this editor:

> *"For **manual editing**, this output can be used as a guide, e.g. by **snapping to the nearest valid
> location** or by visualizing all valid locations, showing their weights."*

Supporting: Merrell et al. 2011 for the clearance distances already in `Clearance` (0.91 m beside a
bed, 0.76 m in front of a seat, 0.61 m in front of shelving); Gregory, *Game Engine Architecture* 3e
§7.2.1 for why the derived form is build output and the authored form is the source of truth.

---

## 3. The model

**One relation: an attachment.** A piece declares the features it **needs**; a piece declares the
features it **offers**; a placement records **which feature instance it took**. There is no second
route.

The move that makes this cover everything: **the room offers features too.** A room is not a special
case outside the system, it is the outermost provider.

| Provider | Offers | Where |
|---|---|---|
| the room | `floor` | `map.origin.1`, everywhere in bounds |
| the room | `ceiling` | `origin.1 + bounds.1` |
| a floor tile | `floor` | its own top — it *provides* floor for its cell |
| a wall segment | `face` ×2 | its two vertical sides, over a height range |
| a table | `worktop` | its drawn top |
| a doorway | `opening` | the hole, with its measured clear size |

Then every current `Mount` variant is the same thing:

| Today | Becomes |
|---|---|
| `OnFloor` | needs `floor` |
| `Tiled` | needs `floor`, **and offers `floor`** |
| `OnWall { height }` | needs `face`, at `height` **along that face** |
| `OnCeiling` | needs `ceiling` |
| `OnSurface { class }` | needs `class` (unchanged — this is the shape the rest adopts) |
| `InOpening` | needs `opening` |
| `Overlay { on }` | needs a plane feature, claims no volume |

### What this fixes, in the same order as §1

1. **A floor tile is not an object standing on the floor — it *is* floor.** It needs `floor` (from the
   room) and offers `floor` (at its own top). A wall needs `floor` and takes it from *the tile*. So
   they never contest: one provides, the other consumes. That is the whole "four meshes in one grid
   unit" confusion, resolved by saying which of them is the ground.
2. **`Tiled` has nowhere to be dumped**, because there is no stratum enum any more. A barrel needs
   `floor` like everything else and contests the floor like everything else.
3. **Every piece has a host**, so deleting a wall can report the sconces that were on it — the same
   way `stack::group_of` already reports what rides a table.

### The overlap rule stops being a table

Two pieces contest space iff they took **the same feature instance** — the room's floor, or
`wall@3`'s north face, or `table@4`'s top. Not a hand-maintained match over variant pairs; a property
of the attachment. Tutenel's off-limits/clearance distinction becomes the feature's own type, so
`Clearance` folds in rather than sitting beside it.

### What must not be lost

Everything the relational half already gets right, because it is what the rest is being moved onto:

- **Height is derived, never authored** — a host's *drawn* top, `scale` and `stretch_y` included.
- **A reference, not a value** — move the host and the guest follows.
- **Class matching through the vocabulary**, so a misspelling is refused at library load for everyone
  at once rather than by failing to stack.
- **Loops detected and named**, not recursed into.
- **`Placed::lift`** stays: the one authored amendment on top of a derived Y.

---

## 4. Blast radius

Measured, not guessed. This is why it is a design and not a branch.

| Touches | What |
|---|---|
| `emerge-core/src/descriptor.rs` | `Mount`, `Offers`, `Socket`, `Clearance` — the schema itself |
| `emerge-core/src/stack.rs` | `datum`, `resolve_y`, `placement_at`, `blocking`, `same_layer`, `plan_box` |
| `emerge-core/src/adjacency.rs` | edge tokens read off boundary cells |
| `emerge-core/src/map.rs` | `Placed::on` grows a feature name |
| `emerge-core/src/placement/` | the solvers that place by mount |
| `emerge-mapper` | the ghost, the fill, `H`'s target stack, the Tiles lattice |
| `src/emerge_map.rs` | the game's loader |
| **every shipped library RON** | 75 + 45 + 41 + 4 descriptors |
| **the goldens** | anything that moves a placement moves `snapshot_hash` |

### Staging

1. **Add features alongside `Mount`**, resolving through the existing code. Nothing changes
   behaviourally; the schema gains a way to say the new thing.
2. **Convert one kit** — the site kit, where the wall/floor defect lives — and prove the three defects
   in §1 go with it.
3. **Derive `same_layer` from attachments** and delete the table.
4. **Retire the positional `Mount` variants** once nothing authors them.

Each step is a green suite; step 3 is where a golden is expected to move, and it should be re-measured
rather than re-pinned blind.

---

## 5. Open questions for the author

1. **Does a wall offer one face feature or two?** Two is honest (a corridor wall has a sconce on each
   side) and doubles the feature count.
2. **Should the room's `floor` be one feature or one per cell?** One per cell makes "this cell is
   floored by a tile" expressible directly and makes the overlap test local; one plane is simpler and
   pushes that into the tile.
3. **Is `Socket` a feature too?** It is already an attachment point with a role
   (`Offers::sockets`, consumed by `smart::seats_of`). Folding it in would mean one concept rather
   than two — but its own note says *"there is no socket type here, and that is deliberate"* about
   compositions, and that argument should be re-read before overturning it.
4. **What reports an orphan?** Once a sconce has a host, deleting the wall can refuse, cascade, or
   report. The map already refuses to load with a dangling `on`, so "refuse" is the existing answer
   and probably the right one.
