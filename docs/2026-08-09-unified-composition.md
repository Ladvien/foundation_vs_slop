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

## 3. Superseded — read `research/2026-08-09-grid-composition-corpus-check.md` first

Everything above (the measurement in §1, the Tutenel framing in §2) stands. The model that used to
follow it does not, and neither did four of its five open questions.

The design that replaced it is **lattice composition**: a bounded composition IS a tile, authored on
the Compose tab by seating meshes into a subdivided volume, and stamped onto the map as one unit. The
map becomes a grid of tile references; the relation between floor, wall and sconce is baked into the
tile's local coordinates at authoring time rather than resolved per-placement at runtime.

`Envelope::Bounded` already does most of this. `composition::interface` builds a scratch map whose
bounds ARE the tile — *"the envelope becomes one: a floor at zero, the declared bounds, and the members
on it, which means `stack::resolve_y` answers here exactly as it will in the game"* — resolves the
members, and reads edge tokens off their boundary cells. What is missing is the authoring surface and
the decision to make it primary.

### What the corpus settled

Read the corpus check for the citations; the conclusions that change this document:

**The vertical friction was imaginary.** CGA Shape (Müller et al. 2006, `10.1145/1179352.1141931`)
types split values as **absolute or relative**: `Subdiv("Y", 1.8, 1r)` keeps `1.8` at 1.8 on a 3 m tile
and on a 4 m one. A layer index is *derived from the split*, never authored, so there is no "1.8 m
becomes 5.4 of 9 layers" conversion to round. `OnWall { height: 1.8 }` is an absolute split value and
"third band up" is a relative one — one mechanism, two value types, the type declared where the number
is written.

**The two levels are a dimension reduction, not a finer grid.** CGA's component split takes a 3-D scope
to 2-D faces to 1-D edges. The 3-D scope seats meshes; **the 2-D face is where an interface token
belongs**; the 1-D edge is where corner agreement belongs. That is the fix for `summarise_face`'s noise
at the root: a face is one component however finely its interior subdivides, so divisions stop leaking
into the adjacency vocabulary. Merrell & Manocha (`10.1109/tvcg.2010.112`) add the cost argument against
a uniform 9×9 — small objects need closely spaced planes, large ones need volume, and doing both
uniformly means "many planes must be created".

**The same authors warn where the lattice stops.** *"the strict hierarchy of the split-grammar can no
longer be enforced… we did not find it suitable for many forms of mass modeling."* The lattice is right
inside a tile and wrong as the map's mechanism. This design already splits those; the citation says the
split is not optional.

**§5 was a false dilemma.** Per-cell and one-plane answer different questions. **Ownership** is
per-cell — Tutenel's typed features and Infinigen's `StableAgainst(Tag.Bottom, Tag.Floor)` both make
"this cell is floored by a tile" a property the tile carries, which keeps the overlap test local.
**Alignment** is the global plane registry — CGA's construction planes and snap lines, which is what
makes *"the floor levels automatically aligned over all solids"*. Take both. Conflating them is what
made the question feel blocking.

**The cross-product does not have to be authored.** Karth & Smith's multi-tile modules via edge
constraints (`10.1145/3337722.3341845` fn. 11), Sturgeon's tags and functional/image grid split
(`10.1609/aiide.v18i1.21944`), and CGA's query-time `Shape.occ(...) ~> door`. So: nest at authoring
time when the arrangement is one artistic decision that should always travel together; compose at stamp
time when it is a cross product. Both, doing different jobs.

**Free placement stays.** No dissent anywhere in the corpus — Merrell 2011, Infinigen and Tutenel all
place continuously. `Placed::at` is `(f32, f32)` and stays that way. Structure is gridded; dressing is
not.

### Two things to write down before code, both prospective

**The self-occlusion trap is ahead of us, not behind.** CGA's Figure 2 is exactly this project's "four
meshes in one grid unit" report — *"several unwanted intersections will cut windows in unnatural ways,
as the volumes are not aware of each other"* — and their fix was an occlusion query with explicit
scoping, of which `Shape.occ("noparent")` is the one that matters: *"we avoid the querying of parent
shapes, which, in the case of a split, always occlude their successor shapes."*

Checked against source on 2026-08-09: **`composition::interface` never calls the overlap rule at all** —
no `blocking`, no `plans_overlap`, no `same_layer`. Its faults are token disagreements between members.
So the trap does not bite today, and it will the moment the lattice adds an occupancy test, which it
must. Write the exclusion when the test is written, not after.

**We have the corner problem, by construction.** `emerge_core::adjacency` reads tokens per boundary cell
on a face — colored edges, i.e. Wang tiles. Lagae & Dutré (2006), `10.1145/1183287.1183296`: *"Wang
tiles do not directly constrain their diagonal neighbors. This leads to continuity problems near tile
corners, a problem commonly known as the corner problem. Corner tiles, on the other hand, do impose
restrictions on their diagonal neighbors."* Four tiles can each satisfy every edge constraint and still
disagree where they meet. Corner tiles are also *"easier to tile"* and halve the memory. This is the
same place CGA's 1-D component points at — two independent sources on one gap.

**Not open access**; the abstract above is from CrossRef. `paper_download` failed on 2026-08-09 with no
OA PDF found. Worth getting through another route before the interface format is fixed, because whether
a token lives on an edge or a corner is a schema decision and not an easy one to revisit.

---

## 4. Blast radius

Unchanged from the measurement, and it is why this is a design and not a branch.

| Touches | What |
|---|---|
| `emerge-core/src/composition.rs` | `Envelope`, `Member`, `interface` — the authoring unit |
| `emerge-core/src/descriptor.rs` | `Mount` becomes split values; `subgrid` becomes the seating lattice |
| `emerge-core/src/stack.rs` | `datum`, `resolve_y`, `blocking`, `same_layer` |
| `emerge-core/src/adjacency.rs` | tokens move from cells to components; edge-vs-corner is open |
| `emerge-mapper` | the Compose tab gains the lattice authoring surface it was always missing |
| `src/emerge_map.rs` | the game's loader |
| **every shipped library RON** and **the goldens** | |

### Staging

1. **The Compose tab authors a `Bounded` composition into a lattice.** No schema change: seat existing
   members at existing mounts, and let `interface` keep deriving what it already derives.
2. **Interface tokens move from cells to bands.** ~~the 2-D component~~ — measured and corrected: four
   shipped pieces present `wall` at the jambs and nothing through the opening, so one token per side
   would have to fault every doorway. A face is its *rectangles* instead, canonical because they are
   taken strips-then-runs. `summarise_face`'s noise goes away with the cell counts that caused it, and
   `adjacency::face` turned out to belong to `grammar::learn`'s prototype signature rather than to
   this. Independent of everything else, and done.
3. **Split values gain the absolute/relative type**, so a member's Y is one mechanism.
4. **The occupancy test arrives, with `noparent` scoping written at the same time.**
5. **Retire the positional `Mount` variants** once nothing authors them.

Each step is a green suite. Step 4 is where a golden is expected to move.

---

## 5. What is still unproven

The corpus argues about **mechanism** and says nothing about whether this project's kits survive it.
Named honestly because it is the real risk:

- **Nothing in the corpus covers nested tile authoring for game kits.** The nearest hit partitions to
  shrink a quantum circuit, which neither supports nor contradicts this.
- **The inversion — a floor tile *provides* floor rather than standing on it — is proved only against
  the site kit's failure.** The furniture kit and the WFC solvers are untouched by the sweep and by this
  document.
- **Edge tokens vs corner tokens is open**, and the paper that settles it is not in the library.

## 6. Open, for the author

1. Does a wall offer one face component or two? Two is honest — a corridor wall takes a sconce on each
   side.
2. Is `Offers::sockets` a component too? Its own note says *"there is no socket type here, and that is
   deliberate"*; that argument should be re-read before overturning it.
3. What reports an orphan? The map already refuses to load with a dangling `on`, so "refuse" is the
   existing answer and probably the right one.
