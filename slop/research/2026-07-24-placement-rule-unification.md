# Unifying placement rules — what exists, what the research says, and the plan

**Trigger:** player capture `region_2026-07-24_14-21-50-393` — "We need to make sure TVs don't spawn on
beds. Add a WFC rule."

Two corrections up front, because they shape everything below:

1. **This is not WFC.** WFC handles `role: Tiled` clutter. A TV is `role: Scatter`, placed by
   `scatter::scatter_on_surfaces`, which — per its own module doc, `src/placement/scatter.rs:13` — the
   furnish pass "calls **directly**", bypassing the solver stack entirely.
2. **The rule system you're asking for already exists.** It's `src/placement/ir.rs`. The scatter pass just
   never got compiled into it. That gap *is* the bug.

---

## 1. What exists today

Three layers, only two of which are joined up.

### Layer 1 — the manifest vocabulary (`assets/config/config.ron`, `manifest.rs`)

Opaque tokens per asset, deliberately kit-agnostic: `tags` (room class), `affordances` (what it is *for*),
`role` (how it is placed), `footprint`, `height`, `pivot`.

### Layer 2 — the constraint IR (`src/placement/ir.rs`)

Engine-free, `Serialize`/`Deserialize`, and already well-shaped:

```rust
Candidate { asset, role, footprint, dof, affordances }
Constraint { scope, predicate, modality, guard }
Scope     ::= Object | Pair | Group | Region
Predicate ::= Clearance(m) | AgainstWall | Facing(ix) | MinDistance(m)
            | Near(m) | Count{tag,count} | Aligned(name) | Custom(token)
Modality  ::= Hard | Soft(weight)
Guard(String)                       // applicability condition
```

### Layer 3 — solver backends (`solver.rs`, `solvers/`)

Routed by capability profile (`Hardness` × `Locality`), first cover wins:

| Backend | Profile | Used for |
|---|---|---|
| `WfcSolver` | Hard + Local | tiled clutter |
| `MetropolisSolver` | Soft + Relational | freestanding layout |
| `ConstraintSolver` | Hard + Global + Cardinality | counts, one-door-per-room |

### The gap

`furnish.rs` runs **four passes**, and only two reach the orchestrator (`furnish.rs:813`,
`orchestrator.solve_group`). Pass 4 — scatter props on support surfaces — instead uses a hardcoded
bitmask (`furnish.rs:124-159`):

```rust
const SURFACE_SUPPORT: u32 = 1 << 0;   // any support top (bed/drawer/table/desk)
const SURFACE_WORKTOP: u32 = 1 << 1;   // a desk/table worktop only
```

A prop declares **one** required bit; a surface provides the OR of its bits; a match is
`provides & requires != 0`. That is the entire expressive power. There is no way to say *"not a bed"* —
only *"needs a bit that a bed happens to lack"*.

So the vocabulary is doing two incompatible jobs at once: `"support"` means both **"is a horizontal
surface"** (true of a bed) and **"is a surface props belong on"** (false of a bed). The TV asked for the
first and got placed by the second.

**The intent was already right and already tested.** `scatter.rs` has
`no worktop -> no lamp (never falls back onto a bed)` — a desk lamp requires `worktop`, so a bed can never
host one, verified over 32 seeds. The TV simply requires the generic `support`. Three other props (globe,
two potted plants) have the same problem and can also land on a bed today.

---

## 2. What the corpus says

### Tutenel et al. 2010 — *A Semantic Scene Description Language for Procedural Layout Solving Problems*, AIIDE. `10.1609/aiide.v6i1.12398`

The paper this codebase already cites for "support is a surface feature". Read properly, it says more than
we implemented:

- Objects carry typed **features** — "generic shapes associating semantics to an object's model". Not one
  flat token list: `top`, `front`, `back`, `storage` (a bookcase has one per shelf).
- **Two feature types carry built-in layout semantics**: *off-limits* features "cannot overlap any other
  features", and *clearance* features "can only overlap other clearance features".
- **Relations are stated between features, not objects**: "place a computer on the **top** feature of a
  desk, with the **front** feature of the screen facing the office chair."
- A **class hierarchy** (WordNet-derived) so relations are declared once and inherited. State a rule on
  `Screen` and every TV/monitor in every kit gets it.
- **Services** — "the capacity of an object to perform a particular action" — are queryable:
  *"some object that provides (the service) heating"*. This is our `affordances`, and note it is kept
  **separate** from features. An object's *purpose* and its *mountable surfaces* are different axes.

That separation is precisely what we collapsed. `"support"` is living in the affordance list, where it is
read as a service.

### Raistrick et al. 2024 — *Infinigen Indoors*, arXiv `2406.11824`

The modern, code-level form of the same idea, and the closest match to our architecture:

```python
on_floor = cl.StableAgainst(Tag.Bottom, Tag.Floor)
constraints = cl.forall(rooms[Tag.DiningRoom], r, (
    tables.related_to(r, on_floor).count() == 1,
    chairs.related_to(r, on_floor).count().in_range(2, 10)))
score = (chairs.related_to(rooms[Tag.DiningRoom], on_floor)
         .mean(c, cl.reflection_symmetry(c, tables) + cl.min_distance(t, tables) * -1)
         .maximize(weight=2))
```

- A relation is a **tag pair**: `StableAgainst(child_part_tag, parent_part_tag)`. This is the exact
  generalisation of "media_surface" — but as a *relation over two tags*, not a new bit.
- Sets are built by **filtering on semantics and scene-graph relations**, which "allows the user to create
  scoped constraints that apply only to objects attached to specific surfaces or rooms."
- Hard constraints and weighted score terms coexist in one graph. Their residential spec: **11 hard, 25
  soft, 1058 graph nodes**, about **15 constraints (~15 lines) per room type**.
- Solved by **simulated annealing with a Metropolis-Hastings acceptance criterion** — which is what
  `MetropolisSolver` already is.
- Stated design goal: *"separates constraint specification from constraint solving"* — the same seam
  `ir.rs` was built around.
- Their constraint families are worth stealing wholesale as a checklist: symmetry, spatial relation,
  quantity, physics (no overhang), **accessibility** (free space in front of appliances).

### Cooper 2022 — *Sturgeon*, AIIDE. `10.1609/aiide.v18i1.21944`

Answers the "how do we handle our WFC rules" half directly:

- A deliberately **tiny mid-level constraint API** — essentially `MAKEVAR`, `MAKECONJ`,
  `CNSTRCOUNT(vars, lo, hi, weight)`, `CNSTRIMPLIESDISJ(lit, disj, weight)`, `SOLVE` — that can be
  instantiated on **different low-level solvers** (SAT, SMT, Answer Set). Same shape as our `Solver`
  trait + `Orchestrator`.
- **Every constraint carries a weight**, so hard and soft are one mechanism, not two.
- **WFC is not a separate system.** Learned adjacency patterns, designer rules, distribution rules and
  reachability rules are all *constraint families in one problem*. (Precedent: Karth & Smith 2017
  implementing WFC in ASP — already cited in our `ir.rs` header.)
- **Tags limit what may be placed where**, and "the tile/tag distinction is intentionally blurred:
  functional tiles can be used as tags to constrain image tile placement."

---

## 3. The convergent design

All three agree on the same three things, and we have partial credit on each:

| Idea | Papers | Us today |
|---|---|---|
| Rules target **tags/features**, never asset keys | all three | ✅ affordances + tags exist |
| A relation is over a **(child tag, parent tag) pair** | Tutenel features; Infinigen `StableAgainst` | ❌ collapsed to a 2-bit mask |
| **One weighted constraint problem**, many solver backends | Sturgeon; Infinigen | ✅ in `ir.rs`, ❌ scatter bypasses it |
| Purpose (*service*) is a **different axis** from mountable surface (*feature*) | Tutenel | ❌ both in `affordances` |
| A **tag hierarchy** so rules inherit across kits | Tutenel (WordNet) | ❌ flat |

---

## 4. The plan

Four stages, each independently shippable and each retiring a hardcoded mechanism.

### Stage 0 — split the two axes (fixes the bug)

Stop overloading `affordances`. A manifest entry gains an explicit surface declaration:

```ron
( key: "bed_double", affordances: ["sleep"],  surfaces: [] )
( key: "drawer",     affordances: ["store"],  surfaces: ["shelf", "media"] )
( key: "desk",       affordances: ["work"],   surfaces: ["shelf", "media", "worktop"] )
( key: "tv",         role: Scatter(surface: "media") )
```

A bed has no `surfaces`, so nothing rests on it — the TV, the globe and both plants are all fixed by one
change, and a future kit cannot reintroduce the bug by tagging a bed `"support"`, because `"support"` is
no longer a surface word. `surface_bits` becomes a small interned-token map rather than two consts.

This is Tutenel's service/feature split, minus the geometry.

### Stage 1 — scatter routes through the IR

Add `Predicate::SupportedBy { child: String, parent: String }` (Infinigen's `StableAgainst`), compile Pass
4 into `Constraint`s, and let the orchestrator route it. The bitmask disappears; `scatter_on_surfaces`
becomes a backend, not a side door. Its existing tests carry over unchanged as backend tests.

Now "no TV on a bed" is expressible three ways — a required surface, a `Hard` negative constraint, or a
`Soft` penalty — instead of only by bit-juggling.

### Stage 2 — typed features with layout semantics

Promote surfaces from a name to a **feature** with a rect and a type, and adopt Tutenel's two special
types: `off_limits` (overlaps nothing) and `clearance` (overlaps only other clearance). We already have
`Predicate::Clearance(f32)` as a scalar; this makes it a real region.

Immediate wins beyond furniture: doorway keep-clear (currently a hardcoded band,
`furnish.rs:693`), the "don't block the entrance" rule, and Infinigen's *accessibility* family
("free space in front of all appliances").

### Stage 3 — a tag hierarchy

`bed ⊂ sleep_surface ⊂ furniture`; `tv ⊂ screen ⊂ appliance`. Rules declared on a parent apply to every
child, in any kit — Tutenel's WordNet library, minus WordNet. This is what makes the vocabulary
extensible rather than merely long.

### Stage 4 — one problem, many families (the WFC unification)

Per Sturgeon: stop treating WFC adjacency as its own mechanism. Tiled patterns, designer rules,
cardinality and reachability all become constraint families over one weighted problem; the orchestrator
already routes by capability. `Modality::Soft(f64)` is already Sturgeon's `weight`.

**This is also the RL/QD hook.** Constraint *weights* are exactly the kind of continuous dial
`squad_ai::level_genome` already evolves — so "how strongly should a sofa face its TV" becomes a searched
parameter rather than the hand-set `w_facing: 1.5` in `config.ron`. Sturgeon's whole point is that learned
and designed constraints live in the same problem.

---

## 5. Beyond furniture

The reason to do this rather than add a `"media"` bit: the same IR already covers things the game solves
ad hoc elsewhere. Creature spawn placement (`enemy::spawn_enemies`, `crab::setup::spawn_crabs`,
`scp999::spawn_scp999`) is three copies of one greedy far-from-spawn scan — that is
`Predicate::MinDistance` + `Predicate::Count` over `Scope::Region`, which the `ConstraintSolver` backend
already handles. Nest seating, almond-water spring spacing and mold seed sites are the same shape.

That is the honest argument for the unified vocabulary: not that a TV on a bed is worth this much
machinery, but that we are currently writing the fifth hand-rolled placement rule while a solver-routed
constraint IR sits unused next to it.

---

## References (all read from the local corpus)

- Tutenel, Smelik, Bidarra, de Kraker (2010). *A Semantic Scene Description Language for Procedural Layout
  Solving Problems.* AIIDE. `10.1609/aiide.v6i1.12398`
- Raistrick et al. (2024). *Infinigen Indoors: Photorealistic Indoor Scenes using Procedural Generation.*
  arXiv `2406.11824`
- Cooper (2022). *Sturgeon: Tile-Based Procedural Level Generation via Learned and Designed Constraints.*
  AIIDE. `10.1609/aiide.v18i1.21944`
- Karth & Smith (2017). *WaveFunctionCollapse is Constraint Solving in the Wild.* FDG — cited in
  `ir.rs`'s header; the precedent for Stage 4.
